package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"os/exec"
	"strconv"
	"testing"
	"time"
)

// This helper has a real controlling terminal, including when stdout is piped.
// A separate gate lets the harness inspect each live frame before prompting.
func TestTerminalProgressPTYHelper(t *testing.T) {
	action := os.Getenv("OA_TEST_PROGRESS_PTY")
	if action == "" {
		return
	}
	fd, err := strconv.Atoi(os.Getenv("OA_TEST_PROGRESS_GATE_FD"))
	if err != nil {
		t.Fatal(err)
	}
	gate := os.NewFile(uintptr(fd), "progress-test-gate")
	defer gate.Close()
	waitForHarness := func() {
		t.Helper()
		if _, err := io.ReadFull(gate, make([]byte, 1)); err != nil {
			t.Fatal(err)
		}
	}
	ui := &terminalSetupPrompter{out: os.Stdout}
	defer ui.Close()
	live := action != "dumb" && action != "redirected"
	if ui.interactiveOutput() != live {
		t.Fatalf("interactive output=%v, want %v", ui.interactiveOutput(), live)
	}
	ui.Printf("PTY_HISTORY_SENTINEL\n")
	ui.Payment("Funding address: before\nSend: 1 ETH\n")
	ui.Progress("Waiting for ETH.")
	// Identical polling data must not add lines to a redirected transcript.
	ui.Payment("Funding address: before\nSend: 1 ETH\n")
	ui.Progress("Waiting for ETH.")
	waitForHarness()
	ui.Payment("Funding address: after\nSend: 2 ETH\n")
	ui.Progress("Watching updated payment.")
	waitForHarness()
	ctx := context.Background()
	switch action {
	case "ask":
		answer, err := ui.Ask(ctx, "PTY progress prompt", "")
		if err != nil || answer != "ready" {
			t.Fatalf("answer=%q, err=%v", answer, err)
		}
	case "continue":
		approved, err := ui.Continue(ctx, "PTY progress prompt")
		if err != nil || !approved {
			t.Fatalf("approved=%v, err=%v", approved, err)
		}
	case "secret":
		answer, err := ui.Secret(ctx, "PTY progress prompt")
		if err != nil || answer != "private-progress-password" {
			t.Fatalf("secret read failed: %v", err)
		}
	case "dumb", "redirected":
		ui.ClearProgress()
	default:
		t.Fatalf("unexpected PTY action %q", action)
	}
	fmt.Println("PTY_EXPECTED_PROGRESS_RESULT")
}

func TestTerminalProgressPTYUpdatesAndPausesForPrompts(t *testing.T) {
	python, err := exec.LookPath("python3")
	if err != nil {
		t.Skip("Python 3 is needed for the real PTY regression")
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	const harness = `
import errno, fcntl, os, pty, re, select, signal, struct, sys, termios, time

action = sys.argv[2]
live = action not in ("dumb", "redirected")
gate_read, gate_write = os.pipe()
output_read, output_write = os.pipe()
os.set_inheritable(gate_read, True)
pid, terminal = pty.fork()
if pid == 0:
    os.close(gate_write)
    os.close(output_read)
    fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 100, 0, 0))
    if action == "redirected":
        os.dup2(output_write, 1)
    os.close(output_write)
    env = dict(os.environ, OA_TEST_PROGRESS_PTY=action,
               OA_TEST_PROGRESS_GATE_FD=str(gate_read),
               TERM="dumb" if action == "dumb" else "xterm-256color")
    os.execve(sys.argv[1], [sys.argv[1], "-test.run=^TestTerminalProgressPTYHelper$"], env)
os.close(gate_read)
os.close(output_write)
sources = [terminal, output_read]
output = b""
reaped = False

def read_output():
    global output
    for source in select.select(sources, [], [], 0.05)[0]:
        try:
            chunk = os.read(source, 65536)
        except OSError as error:
            if error.errno != errno.EIO:
                raise
            chunk = b""
        if chunk:
            output += chunk
        else:
            sources.remove(source)

def wait_for(marker):
    deadline = time.monotonic() + 5
    while marker not in output:
        read_output()
        if time.monotonic() >= deadline:
            raise RuntimeError("expected marker missing: " + repr(marker) + " " + repr(output))

def read_for(seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        read_output()

try:
    wait_for(b"Waiting for ETH.")
    initial = len(output)
    read_for(0.5)
    if live:
        if len(output) == initial or b"\x1b[J" not in output:
            raise RuntimeError("live terminal did not animate and erase its old region: " + repr(output))
    elif len(output) != initial or b"\x1b" in output:
        raise RuntimeError("plain output animated or contained terminal controls: " + repr(output))
    if not live and (output.count(b"Funding address: before") != 1 or output.count(b"Waiting for ETH.") != 1):
        raise RuntimeError("identical polling data repeated in plain output: " + repr(output))
    os.write(gate_write, b"x")
    wait_for(b"Watching updated payment.")
    if b"Funding address: after" not in output:
        raise RuntimeError("payment details did not update: " + repr(output))
    if live and not re.search(rb"\r\x1b\[\d+A\x1b\[JFunding address: after", output):
        raise RuntimeError("updated payment appended instead of replacing the live region: " + repr(output))
    os.write(gate_write, b"x")
    if live:
        wait_for(b"PTY progress prompt: ")
        paused = len(output)
        read_for(0.5)
        if len(output) != paused:
            raise RuntimeError("animation wrote while the prompt owned the terminal: " + repr(output[paused:]))
        if action == "secret" and termios.tcgetattr(terminal)[3] & termios.ECHO:
            raise RuntimeError("password prompt left echo enabled")
        response = {"ask": b"ready\n", "continue": b"\n", "secret": b"private-progress-password\n"}[action]
        os.write(terminal, response)
    wait_for(b"PTY_EXPECTED_PROGRESS_RESULT")
    deadline = time.monotonic() + 3
    while True:
        read_output()
        exited, status = os.waitpid(pid, os.WNOHANG)
        if exited:
            reaped = True
            break
        if time.monotonic() >= deadline:
            raise RuntimeError("progress helper did not exit")
    if os.waitstatus_to_exitcode(status) != 0:
        raise RuntimeError("progress helper failed: " + repr(output))
    if b"private-progress-password" in output:
        raise RuntimeError("password appeared in terminal output")
    if not termios.tcgetattr(terminal)[3] & termios.ECHO:
        raise RuntimeError("terminal echo was not restored")
    if not live and b"\x1b" in output:
        raise RuntimeError("plain output contained cursor controls: " + repr(output))
finally:
    if not reaped:
        try:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        except ProcessLookupError:
            pass
    os.close(gate_write)
    os.close(output_read)
    os.close(terminal)
`
	for _, action := range []string{"ask", "continue", "secret", "dumb", "redirected"} {
		t.Run(action, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
			defer cancel()
			command := exec.CommandContext(ctx, python, "-c", harness, executable, action)
			if output, err := command.CombinedOutput(); err != nil {
				t.Fatalf("live progress PTY: %v\n%s", err, output)
			}
		})
	}
}
