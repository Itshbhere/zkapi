package main

import (
	"context"
	"errors"
	"math/big"
	"strings"
	"testing"

	"github.com/OpenAnonymity/zkapi/zkapi-clientd/internal/zkapi"
)

// Capture the live display separately from the permanent transcript so the
// polling tests exercise the interactive contract, as well as the plain fallback.
type recordingSetupDisplay struct {
	fundingWizardUI
	progress         []string
	payments         []string
	active           string
	promptDuringWait bool
}

func (u *recordingSetupDisplay) Progress(message string) {
	u.progress = append(u.progress, message)
	u.active = message
}

func (u *recordingSetupDisplay) Payment(payment string) {
	u.payments = append(u.payments, payment)
}

func (u *recordingSetupDisplay) ClearProgress() { u.active = "" }

func (u *recordingSetupDisplay) Printf(format string, args ...any) {
	u.ClearProgress()
	u.fundingWizardUI.Printf(format, args...)
}

func (u *recordingSetupDisplay) Continue(ctx context.Context, question string) (bool, error) {
	u.promptDuringWait = u.promptDuringWait || u.active != ""
	return u.fundingWizardUI.Continue(ctx, question)
}

func TestGuidedDepositRefreshesPaymentInPlaceAndClearsCancelledWait(t *testing.T) {
	f := newWizardFixture()
	u := &recordingSetupDisplay{}
	initial := wizardQuote()
	initial.BalanceWei = "0"
	initial.ShortfallWei = initial.RequiredTotalWei
	initial.RecommendedTopUpWei = initial.RecommendedTotalWei
	f.quote = func(call int, amount, usd uint64) (zkapi.AddressPaymentQuote, error) {
		q := initial
		// Network fees change while the receiving account remains unfunded.
		increase := big.NewInt(int64(call * 1000))
		for _, field := range []*string{&q.RequiredFeeWei, &q.RequiredTotalWei, &q.ShortfallWei, &q.FeeReserveWei, &q.RecommendedTotalWei, &q.RecommendedTopUpWei} {
			value, _ := new(big.Int).SetString(*field, 10)
			*field = value.Add(value, increase).String()
		}
		return q, nil
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	waits := 0
	err := waitAndDeposit(ctx, f, initial, u, func(ctx context.Context) error {
		waits++
		if !strings.Contains(u.active, "Waiting for ETH:") {
			t.Fatal("polling has no visible funding status")
		}
		if waits == 2 {
			cancel()
		}
		return ctx.Err()
	})
	if !errors.Is(err, context.Canceled) || u.active != "" || f.approveCalls != 0 || u.continues != 0 {
		t.Fatalf("cancelled funding wait did not stop cleanly: %v, %+v", err, u)
	}
	if len(u.payments) != 3 || strings.Contains(u.String(), "Scan with") || strings.Contains(u.String(), "Payment information updated") || strings.Contains(u.String(), "Waiting for ETH:") {
		t.Fatal("polling appended payment cards or status to the permanent transcript")
	}
	if !strings.Contains(u.payments[2], "Payment information updated") || !strings.Contains(u.payments[2], "Payment URI:") {
		t.Fatal("refreshed payment lost its usable transfer instructions")
	}
}

func TestGuidedDepositRestoresFundingStatusAfterQuoteRetry(t *testing.T) {
	f := newWizardFixture()
	u := &recordingSetupDisplay{}
	initial := wizardQuote()
	initial.BalanceWei = "0"
	initial.ShortfallWei = initial.RequiredTotalWei
	initial.RecommendedTopUpWei = initial.RecommendedTotalWei
	f.quote = func(call int, _, _ uint64) (zkapi.AddressPaymentQuote, error) {
		if call == 1 {
			return zkapi.AddressPaymentQuote{}, errors.New("temporary quote failure")
		}
		return initial, nil
	}
	f.address = func() zkapi.AddressFundingStatus {
		state := wizardState("waiting_funds")
		state.TransactionHash = ""
		return state
	}
	waits := 0
	err := waitAndDeposit(context.Background(), f, initial, u, func(context.Context) error {
		waits++
		if waits == 2 {
			if !strings.Contains(u.active, "Waiting for ETH:") {
				t.Fatal("the recovered quote left a stale retry message visible")
			}
			return context.Canceled
		}
		return nil
	})
	if !errors.Is(err, context.Canceled) || u.active != "" || f.approveCalls != 0 {
		t.Fatal("quote retry changed cancellation or spending behavior")
	}
}

func TestGuidedDepositRestoresStageAfterInterruptedRecovery(t *testing.T) {
	f := newWizardFixture()
	u := &recordingSetupDisplay{}
	initial := wizardState("deposit_pending")
	initial.DepositStage = "finalizing"
	f.resume = func(uint64) (zkapi.AddressFundingStatus, error) {
		if f.resumeCalls == 1 {
			return zkapi.AddressFundingStatus{}, errors.New("temporary response failure")
		}
		return wizardState("active"), nil
	}
	f.address = func() zkapi.AddressFundingStatus { return initial }
	if err := finishGuidedDeposit(context.Background(), f, initial, u, immediateWizardPoll); err != nil {
		t.Fatal(err)
	}
	updates := strings.Join(u.progress, "\n")
	if strings.Count(updates, "Deposit mined;") != 2 || !strings.Contains(updates, "Could not confirm") || u.active != "" {
		t.Fatalf("stage did not recover after the interruption: %s", updates)
	}
	if strings.Contains(u.String(), "waiting") || !strings.Contains(u.String(), "Private inference balance activated") || f.approveCalls != 0 {
		t.Fatal("transient stages entered history or recovery authorized a transaction")
	}
}

func TestGuidedFundingClearsProgressBeforeDepositConsent(t *testing.T) {
	f := newWizardFixture()
	u := &recordingSetupDisplay{fundingWizardUI: fundingWizardUI{confirmations: []bool{false}}}
	err := guidedFunding(context.Background(), f, "20", u, immediateWizardPoll)
	if err == nil || !strings.Contains(err.Error(), "declined") || u.promptDuringWait || u.active != "" || f.approveCalls != 0 {
		t.Fatalf("progress interfered with explicit consent: %v, %+v", err, u)
	}
}

func TestGuidedDepositRestoresWaitAfterFundingFallsBelowApprovedAmount(t *testing.T) {
	f := newWizardFixture()
	u := &recordingSetupDisplay{fundingWizardUI: fundingWizardUI{confirmations: []bool{true}}}
	initial := wizardQuote()
	f.quote = func(call int, _, _ uint64) (zkapi.AddressPaymentQuote, error) {
		quote := initial
		if call > 1 {
			quote.BalanceWei = "0"
			quote.ShortfallWei = quote.RequiredTotalWei
			quote.RecommendedTopUpWei = quote.RecommendedTotalWei
		}
		return quote, nil
	}
	err := waitAndDeposit(context.Background(), f, initial, u, func(context.Context) error {
		if !strings.Contains(u.active, "Waiting for ETH:") {
			t.Fatal("the balance-loss warning left the funding wait blank")
		}
		return context.Canceled
	})
	if !errors.Is(err, context.Canceled) || u.active != "" || f.approveCalls != 0 || u.continues != 1 {
		t.Fatal("balance loss did not revoke the previous deposit consent")
	}
}

func TestGuidedDepositDistinguishesMiningFeeWaitFinalityAndActivation(t *testing.T) {
	f := newWizardFixture()
	u := &fundingWizardUI{}
	initial := wizardState("deposit_pending")
	initial.DepositStage = "pending"
	stages := []struct{ phase, stage string }{
		{"deposit_pending", "fee_wait"},
		{"deposit_pending", "pending"},
		{"deposit_pending", "finalizing"},
		{"deposit_pending", "finalizing"},
		{"confirming", "activating"},
		{"active", "active"},
	}
	f.resume = func(amount uint64) (zkapi.AddressFundingStatus, error) {
		if amount != initial.Amount || f.resumeCalls > len(stages) {
			t.Fatal("recovery changed the deposit or did not finish")
		}
		next := stages[f.resumeCalls-1]
		state := wizardState(next.phase)
		state.DepositStage = next.stage
		state.Message = "untrusted remote details must stay hidden"
		return state, nil
	}
	if err := finishGuidedDeposit(context.Background(), f, initial, u, immediateWizardPoll); err != nil {
		t.Fatal(err)
	}
	output := u.String()
	previous := -1
	for _, want := range []string{"waiting to be mined", "signed fee cap", "Deposit mined; waiting for Ethereum finality", "Deposit finalized; activating", "Private inference balance activated"} {
		index := strings.Index(output, want)
		if index <= previous {
			t.Fatalf("missing or out-of-order progress %q: %s", want, output)
		}
		previous = index
	}
	if strings.Count(output, "Deposit mined;") != 1 || strings.Contains(output, "untrusted remote") || strings.Contains(output, "submitted; waiting") {
		t.Fatal("progress repeated unnecessarily, leaked remote data, or conflated mining with finality")
	}
	if f.approveCalls != 0 || f.resumeCalls != len(stages) {
		t.Fatal("progress changed the recovery authorization")
	}
}

func TestGuidedDepositStatusFailureDoesNotInventLostInclusion(t *testing.T) {
	f := newWizardFixture()
	u := &fundingWizardUI{}
	initial := wizardState("deposit_pending")
	initial.DepositStage = "finalizing"
	f.resume = func(uint64) (zkapi.AddressFundingStatus, error) {
		if f.resumeCalls == 1 {
			return zkapi.AddressFundingStatus{}, errors.New("receipt RPC unavailable")
		}
		return wizardState("active"), nil
	}
	f.address = func() zkapi.AddressFundingStatus {
		// Durable transaction identity is known, but a failed RPC cannot tell
		// us whether the prior inclusion has changed.
		return wizardState("deposit_pending")
	}
	if err := finishGuidedDeposit(context.Background(), f, initial, u, immediateWizardPoll); err != nil {
		t.Fatal(err)
	}
	output := u.String()
	if !strings.Contains(output, "Deposit mined;") || !strings.Contains(output, "Waiting for deposit confirmation;") || strings.Contains(output, "waiting to be mined") || strings.Contains(output, "fees exceed") {
		t.Fatalf("uncertain status misrepresented mining or fees: %s", output)
	}
	if f.resumeCalls != 2 || f.approveCalls != 0 {
		t.Fatal("uncertain progress changed recovery authorization")
	}
}
