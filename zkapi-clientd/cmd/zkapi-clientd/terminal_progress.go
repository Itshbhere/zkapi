package main

import (
	"fmt"
	"os"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"golang.org/x/sys/unix"
)

// Optional presentation capabilities keep wallet operations and test prompters
// independent of the terminal. None of these methods read or authorize input.
func setupProgress(ui setupPrompter, format string, args ...any) {
	message := strings.TrimSpace(fmt.Sprintf(format, args...))
	if live, ok := ui.(interface{ Progress(string) }); ok {
		live.Progress(message)
	} else {
		ui.Printf("%s\n", message)
	}
}

func clearSetupProgress(ui setupPrompter) {
	if live, ok := ui.(interface{ ClearProgress() }); ok {
		live.ClearProgress()
	}
}

func setupPayment(ui setupPrompter, payment string) {
	if live, ok := ui.(interface{ Payment(string) }); ok {
		live.Payment(payment)
	} else {
		ui.Printf("%s", payment)
	}
}

type setupDisplay struct {
	stop                       chan struct{}
	message, payment           string
	plainMessage, plainPayment string
	started                    time.Time
	frame, rows                int
	width, height              int
}

func (p *terminalSetupPrompter) outputTerminalSize() (int, int) {
	if output, ok := p.out.(interface{ Fd() uintptr }); ok {
		if size, err := unix.IoctlGetWinsize(int(output.Fd()), unix.TIOCGWINSZ); err == nil && size.Col > 0 && size.Row > 0 {
			return int(size.Col), int(size.Row)
		}
	}
	return 80, 24
}

func (p *terminalSetupPrompter) interactiveOutput() bool {
	if strings.EqualFold(os.Getenv("TERM"), "dumb") {
		return false
	}
	output, ok := p.out.(interface{ Fd() uintptr })
	if !ok {
		return false
	}
	_, err := unix.IoctlGetWinsize(int(output.Fd()), unix.TIOCGWINSZ)
	return err == nil
}

func (p *terminalSetupPrompter) Progress(message string) {
	p.displayMu.Lock()
	defer p.displayMu.Unlock()
	message = setupPlainText(message)
	if !p.interactiveOutput() {
		if message != p.display.plainMessage {
			_, _ = fmt.Fprintln(p.out, message)
			p.display.plainMessage = message
		}
		return
	}
	if p.display.message == "" {
		p.display.started = time.Now()
		p.display.frame = 0
	}
	p.display.message = message
	if p.display.stop == nil {
		stop := make(chan struct{})
		p.display.stop = stop
		go p.animateSetup(stop)
	}
	p.renderDisplayLocked()
}

func (p *terminalSetupPrompter) Payment(payment string) {
	p.displayMu.Lock()
	defer p.displayMu.Unlock()
	if !p.interactiveOutput() {
		payment = setupPlainText(payment)
		if payment != p.display.plainPayment {
			_, _ = fmt.Fprint(p.out, payment)
			p.display.plainPayment = payment
		}
		return
	}
	p.display.payment = payment
	p.renderDisplayLocked()
}

func (p *terminalSetupPrompter) animateSetup(stop chan struct{}) {
	ticker := time.NewTicker(200 * time.Millisecond)
	defer ticker.Stop()
	for {
		select {
		case <-stop:
			return
		case <-ticker.C:
			p.displayMu.Lock()
			if p.display.stop != stop {
				p.displayMu.Unlock()
				return
			}
			p.display.frame++
			p.renderDisplayLocked()
			p.displayMu.Unlock()
		}
	}
}

// Prompts and durable messages take ownership of the cursor. Commit the latest
// payment details, erase the transient status, and stop all background writes.
func (p *terminalSetupPrompter) ClearProgress() {
	p.displayMu.Lock()
	defer p.displayMu.Unlock()
	p.finishDisplayLocked()
}

func (p *terminalSetupPrompter) finishDisplayLocked() {
	if p.display.stop != nil {
		close(p.display.stop)
		p.display.stop = nil
	}
	if p.display.rows > 0 {
		payment := p.display.payment
		p.display.message = ""
		p.display.payment = ""
		p.renderDisplayLocked()
		// Keep a complete, scrollable copy at a prompt or milestone, including
		// details that could not fit in the live viewport.
		if payment != "" {
			width, _ := p.outputTerminalSize()
			lines := setupPaymentLines(payment, max(1, width-1), 1<<20)
			_, _ = fmt.Fprintln(p.out, strings.Join(lines, "\n"))
		}
	}
	p.display.rows = 0
	p.display.message, p.display.payment = "", ""
	p.display.plainMessage, p.display.plainPayment = "", ""
}

func (p *terminalSetupPrompter) renderDisplayLocked() {
	width, height := p.outputTerminalSize()
	// Leave a spare column and row so neither autowrap nor a bottom-row newline
	// can move the live region beyond the visible terminal.
	columns, rows := max(1, width-1), max(1, height-1)
	lines := setupPaymentLines(p.display.payment, columns, max(0, rows-3))
	if p.display.message != "" {
		elapsed := int(time.Since(p.display.started).Seconds())
		status := fmt.Sprintf("%c %s", "|/-\\"[p.display.frame%4], p.display.message)
		statusLines := setupWrapText(status, columns)
		// The complete message remains available in normal-width terminals.
		// At very small sizes use the remaining rows without wrapping history.
		statusLines = statusLines[:min(len(statusLines), max(1, rows-len(lines)-1))]
		lines = append(lines, statusLines...)
		if len(lines) < rows {
			lines = append(lines, setupClipText(fmt.Sprintf("  %d:%02d elapsed · Ctrl+C to stop", elapsed/60, elapsed%60), columns))
		}
	}
	if len(lines) > rows {
		lines = lines[:rows]
	}
	// Terminal reflow is implementation dependent. After a resize start a new
	// region instead of guessing which earlier rows belong to us and risking
	// erasing a quote, question, or the user's shell history.
	previousRows := min(p.display.rows, rows)
	if p.display.width != width || p.display.height != height {
		previousRows = 0
	}
	var frame strings.Builder
	if previousRows > 0 {
		_, _ = fmt.Fprintf(&frame, "\r\x1b[%dA\x1b[J", previousRows)
	}
	if len(lines) > 0 {
		frame.WriteString(strings.Join(lines, "\n") + "\n")
	}
	_, _ = fmt.Fprint(p.out, frame.String())
	p.display.rows, p.display.width, p.display.height = len(lines), width, height
}

// A QR must be displayed whole. On shorter/narrower terminals retain the text
// payment request and address, and reveal the QR automatically after a resize.
func setupPaymentLines(payment string, width, height int) []string {
	if payment == "" || height == 0 {
		return nil
	}
	raw := strings.Split(strings.TrimSpace(payment), "\n")
	var full, compact []string
	qrFits, hasQR := true, false
	for _, line := range raw {
		if strings.HasPrefix(line, paymentQRColors) {
			hasQR = true
			if utf8.RuneCountInString(setupPlainText(line)) > width {
				qrFits = false
			}
			full = append(full, line)
			continue
		}
		plain := setupPlainText(line)
		full = append(full, setupWrapText(plain, width)...)
		if strings.HasPrefix(plain, "Payment URI: ") || plain == "Scan with an Ethereum wallet:" {
			continue
		}
		compact = append(compact, setupWrapText(plain, width)...)
	}
	if qrFits && len(full) <= height {
		return full
	}
	if hasQR && len(compact) < height {
		compact = append(compact, setupClipText("Enlarge the terminal to show the payment QR.", width))
	}
	// Very small viewports cannot fit a payment. Avoid a partial QR; the
	// full details are available again as soon as the terminal is enlarged.
	if len(compact) > height {
		compact = compact[:height]
		compact[height-1] = setupClipText("Enlarge terminal for full payment details.", width)
	}
	return compact
}

func setupWrapText(text string, width int) []string {
	width = max(1, width)
	runes := []rune(text)
	var lines []string
	for len(runes) > width {
		lines = append(lines, string(runes[:width]))
		runes = runes[width:]
	}
	return append(lines, string(runes))
}

func setupClipText(text string, width int) string {
	runes := []rune(text)
	if len(runes) > width {
		return string(runes[:max(0, width-1)]) + "…"
	}
	return text
}

// Only locally generated QR colors are retained by the renderer. Plain logs
// and status messages cannot inject terminal control sequences.
func setupPlainText(text string) string {
	text = strings.ReplaceAll(strings.ReplaceAll(text, paymentQRColors, ""), paymentQRReset, "")
	return strings.Map(func(r rune) rune {
		if unicode.IsControl(r) && r != '\n' {
			return -1
		}
		return r
	}, text)
}
