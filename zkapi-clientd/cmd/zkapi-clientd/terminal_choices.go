package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"

	"golang.org/x/sys/unix"
)

type setupChoice struct {
	value string
	label string
}

var errSetupSelectionCanceled = errors.New("setup selection canceled")

// Keep scripted prompters and plain output compatible with the existing text
// questions. Only the real terminal prompter opts into keyboard selection.
func selectSetupChoice(ctx context.Context, ui setupPrompter, question, fallback string, choices []setupChoice) (string, error) {
	if selector, ok := ui.(interface {
		Select(context.Context, string, string, []setupChoice) (string, error)
	}); ok {
		return selector.Select(ctx, question, fallback, choices)
	}
	return ui.Ask(ctx, question, fallback)
}

func (p *terminalSetupPrompter) Select(ctx context.Context, question, fallback string, choices []setupChoice) (answer string, result error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if len(choices) == 0 {
		return "", errors.New("setup choice list is empty")
	}
	if !p.interactiveOutput() {
		return p.Ask(ctx, question, fallback)
	}
	if err := p.openTerminal(); err != nil {
		return "", err
	}
	if p.device == nil {
		return p.Ask(ctx, question, fallback)
	}
	restore, err := enableTerminalChoices(p.device.fd)
	if err != nil {
		return "", errors.New("could not enable keyboard selection in setup terminal")
	}
	defer restore()
	p.ClearProgress()
	selected := 0
	for i, choice := range choices {
		if choice.value == fallback {
			selected = i
			break
		}
	}
	width, height := p.outputTerminalSize()
	visible := min(len(choices), max(1, height-3))
	heading := question
	if strings.HasPrefix(heading, "Choose: ") {
		heading = "Choose an action"
	} else if strings.HasPrefix(heading, "Choose network: ") {
		heading = "Choose network"
	}
	drawHeading := func() {
		_, _ = fmt.Fprintf(p.out, "%s\n%s\n\x1b[?25l", truncateSetupChoice(heading, width), truncateSetupChoice("  ↑/↓ or j/k to move · Enter to select · Esc/q to cancel", width))
	}
	drawHeading()
	defer func() {
		// Remove the temporary hint and choice list, retaining a compact record
		// of the question and selected answer in the terminal scrollback.
		if currentWidth, currentHeight := p.outputTerminalSize(); currentWidth == width && currentHeight == height {
			_, _ = fmt.Fprintf(p.out, "\x1b[%dA\r\x1b[J\x1b[?25h", visible+1)
		} else {
			// Resize can reflow prior rows; leave that history alone instead of
			// erasing an unknown number of lines above the current cursor.
			_, _ = fmt.Fprint(p.out, "\r\x1b[2K\x1b[?25h")
		}
		if result != nil {
			_, _ = fmt.Fprintln(p.out, "  Canceled.")
			return
		}
		for _, choice := range choices {
			if choice.value == answer {
				_, _ = fmt.Fprintf(p.out, "  %s\n", choice.label)
				return
			}
		}
	}()
	renderSetupChoices(p.out, choices, selected, visible, width)
	for {
		key, err := p.readSetupChoiceByte(ctx)
		if err != nil {
			return "", err
		}
		if key == '\x1b' {
			// A lone Escape cancels. Arrow keys arrive as ESC [ A/B (or ESC O
			// A/B); a short bounded read distinguishes those from Escape.
			sequence, cancel := context.WithTimeout(ctx, 100*time.Millisecond)
			prefix, prefixErr := p.readSetupChoiceByte(sequence)
			if prefixErr == nil && (prefix == '[' || prefix == 'O') {
				key, err = p.readSetupChoiceByte(sequence)
				cancel()
				if err != nil {
					if ctx.Err() != nil {
						return "", ctx.Err()
					}
					continue
				}
				switch key {
				case 'A':
					key = 'k'
				case 'B':
					key = 'j'
				default:
					continue
				}
			} else {
				cancel()
				if ctx.Err() != nil {
					return "", ctx.Err()
				}
				key = 'q'
			}
		}
		switch key {
		case '\n', '\r':
			if err := ctx.Err(); err != nil {
				return "", err
			}
			return choices[selected].value, nil
		case 'q', 'Q':
			for _, choice := range choices {
				if choice.value == "quit" {
					return "quit", nil
				}
			}
			return "", errSetupSelectionCanceled
		case '\x04':
			return "", errors.New("setup input ended; run zkapi-clientd config again to continue")
		case 'k', 'K':
			selected = (selected + len(choices) - 1) % len(choices)
		case 'j', 'J':
			selected = (selected + 1) % len(choices)
		default:
			continue
		}
		if nextWidth, nextHeight := p.outputTerminalSize(); nextWidth != width || nextHeight != height {
			width, height = nextWidth, nextHeight
			visible = min(len(choices), max(1, height-3))
			_, _ = fmt.Fprint(p.out, "\r\n")
			drawHeading()
		} else {
			_, _ = fmt.Fprintf(p.out, "\x1b[%dA", visible)
		}
		renderSetupChoices(p.out, choices, selected, visible, width)
	}
}

func renderSetupChoices(out io.Writer, choices []setupChoice, selected, visible, width int) {
	first := min(max(0, selected-visible/2), len(choices)-visible)
	for i := first; i < first+visible; i++ {
		prefix := "  "
		if i == selected {
			prefix = "> "
		}
		_, _ = fmt.Fprintf(out, "\r\x1b[2K%s\n", truncateSetupChoice(prefix+choices[i].label, width))
	}
}

func truncateSetupChoice(text string, width int) string {
	runes := []rune(strings.ReplaceAll(strings.ReplaceAll(text, "\n", " "), "\r", " "))
	// Leave the last column unused to avoid terminal autowrap adding rows.
	limit := max(1, width-1)
	if len(runes) <= limit {
		return string(runes)
	}
	return string(runes[:limit-1]) + "…"
}

func (p *terminalSetupPrompter) readSetupChoiceByte(ctx context.Context) (byte, error) {
	for {
		if err := ctx.Err(); err != nil {
			return 0, err
		}
		if p.input.Buffered() == 0 {
			fds := []unix.PollFd{{Fd: int32(p.device.fd), Events: unix.POLLIN}}
			n, err := unix.Poll(fds, 25)
			if errors.Is(err, unix.EINTR) {
				continue
			}
			if err != nil {
				return 0, errors.New("could not read setup terminal")
			}
			if n == 0 {
				continue
			}
		}
		b, err := p.input.ReadByte()
		if errors.Is(err, unix.EINTR) {
			continue
		}
		if errors.Is(err, unix.EAGAIN) || errors.Is(err, unix.EWOULDBLOCK) {
			timer := time.NewTimer(25 * time.Millisecond)
			select {
			case <-ctx.Done():
				timer.Stop()
				return 0, ctx.Err()
			case <-timer.C:
				continue
			}
		}
		if err != nil {
			return 0, errors.New("setup input ended; run zkapi-clientd config again to continue")
		}
		return b, nil
	}
}
