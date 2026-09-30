package main

import (
	"bufio"
	"context"
	"strings"
	"testing"
	"unicode/utf8"
)

func TestSetupProgressPlainOutputDeduplicatesAndOmitsTerminalControls(t *testing.T) {
	var out strings.Builder
	ui := &terminalSetupPrompter{out: &out}
	if ui.interactiveOutput() {
		t.Fatal("a non-terminal writer enabled interactive output")
	}
	payment := "Payment\n" + paymentQRColors + "██  ██" + paymentQRReset + "\nTo: address\n"
	ui.Payment(payment)
	ui.Payment(payment)
	ui.Progress("Waiting for ETH")
	ui.Progress("Waiting for ETH")
	ui.Progress("Funds available")
	ui.Close()
	if strings.Contains(out.String(), "\x1b") || strings.Count(out.String(), "Waiting for ETH") != 1 || strings.Count(out.String(), "To: address") != 1 || !strings.Contains(out.String(), "Funds available") {
		t.Fatalf("plain output contains control codes or repeated status: %q", out.String())
	}
}

func TestSetupPaymentViewportNeverShowsPartialQR(t *testing.T) {
	qr := strings.Repeat(paymentQRColors+strings.Repeat("█", 50)+paymentQRReset+"\n", 26)
	payment := "Funding address: 0x1234567890\nPrivate deposit: 0.01 ETH\nScan with an Ethereum wallet:\n" + qr + "Network: Ethereum Mainnet\nSend: 0.011 ETH\nTo: 0x1234567890\nPayment URI: ethereum:0x1234567890@1?value=11000000000000000\n"
	for _, size := range []struct{ width, height int }{{79, 45}, {79, 20}, {30, 45}, {10, 5}} {
		lines := setupPaymentLines(payment, size.width, size.height)
		if len(lines) > size.height {
			t.Fatal("payment exceeds available rows")
		}
		qrLines := 0
		for _, line := range lines {
			if utf8.RuneCountInString(setupPlainText(line)) > size.width {
				t.Fatalf("payment wraps beyond reserved width: %q", line)
			}
			if strings.Contains(line, paymentQRColors) {
				qrLines++
			}
		}
		if qrLines != 0 && qrLines != 26 {
			t.Fatal("a partial QR was rendered")
		}
		if size.width == 79 && size.height == 45 && qrLines != 26 {
			t.Fatal("a complete fitting QR was omitted")
		}
		if size.width == 79 && size.height == 20 {
			text := strings.Join(lines, "\n")
			for _, want := range []string{"Funding address: 0x1234567890", "Private deposit: 0.01 ETH", "Send: 0.011 ETH", "Enlarge"} {
				if !strings.Contains(text, want) {
					t.Fatalf("compact payment lost %q", want)
				}
			}
		}
	}
}

func TestSetupDisplayCommitsLatestPaymentBeforePrompt(t *testing.T) {
	var out strings.Builder
	ui := &terminalSetupPrompter{out: &out, input: bufio.NewReader(strings.NewReader("cancel\n"))}
	// Exercise cursor rendering without depending on the test runner's terminal.
	ui.display.payment = "Private deposit: 1 ETH\nMaximum network fee: 0.01 ETH\n"
	ui.renderDisplayLocked()
	ui.display.payment = "Private deposit: 1 ETH\nMaximum network fee: 0.02 ETH\n"
	ui.renderDisplayLocked()
	if !strings.Contains(out.String(), "\x1b[2A\x1b[J") {
		t.Fatalf("payment refresh appended instead of replacing: %q", out.String())
	}
	out.Reset()
	ui.display.stop = make(chan struct{})
	stop := ui.display.stop
	ui.display.message = "Waiting"
	answer, err := ui.Ask(context.Background(), "Authorize this fee", "no")
	if err != nil || answer != "cancel" {
		t.Fatalf("prompt: %q, %v", answer, err)
	}
	select {
	case <-stop:
	default:
		t.Fatal("prompt left background animation enabled")
	}
	if ui.display.rows != 0 || ui.display.stop != nil || ui.display.payment != "" || ui.display.message != "" {
		t.Fatal("prompt retained live display state")
	}
	output := out.String()
	if strings.Index(output, "Maximum network fee: 0.02 ETH") > strings.Index(output, "Authorize this fee") || strings.Contains(output, "0.01 ETH") || strings.Contains(output, "Waiting") {
		t.Fatalf("prompt did not commit only the latest quote: %q", output)
	}
}
