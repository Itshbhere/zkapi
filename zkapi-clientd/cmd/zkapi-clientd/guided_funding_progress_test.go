package main

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/OpenAnonymity/zkapi/zkapi-clientd/internal/zkapi"
)

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
