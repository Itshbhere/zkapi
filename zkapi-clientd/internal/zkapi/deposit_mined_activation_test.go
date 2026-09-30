package zkapi

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"
)

func TestNativeMinedActivationStillRequiresCanonicalMatchingDeposit(t *testing.T) {
	for _, manual := range []bool{false, true} {
		for _, change := range []string{"missing", "block", "transaction", "amount", "commitment", "removed", "failed"} {
			name := "saved/" + change
			if manual {
				name = "manual/" + change
			}
			t.Run(name, func(t *testing.T) {
				f := newAddressFixture(t, true)
				first, err := f.fundNative(t, 100000)
				if err != nil {
					t.Fatal(err)
				}
				f.mine(t, first.TransactionHash, change == "failed")
				f.finalized = "0x1"
				receipt := f.receipts[first.TransactionHash]
				event := sampleReceipt(depositRecord{Contract: addressTestVault, Commitment: "0x1234", Amount: 100000})
				switch change {
				case "missing":
					delete(f.receipts, first.TransactionHash)
				case "block":
					f.canonical = "0x" + strings.Repeat("b", 64)
				case "transaction":
					receipt["transactionHash"] = "0x" + strings.Repeat("b", 64)
				case "amount":
					event = sampleReceipt(depositRecord{Contract: addressTestVault, Commitment: "0x1234", Amount: 100001})
				case "commitment":
					event.Logs[0].Topics[2] = "0x" + addressABIWord("0x5678")
				case "removed":
					event.Logs[0].Removed = true
				}
				if change == "amount" || change == "commitment" || change == "removed" {
					receipt["logs"] = event.Logs
				}
				if manual {
					_, _ = f.h.confirm(context.Background(), first.TransactionHash)
				} else {
					_, _ = f.h.FundAddress(context.Background(), 100000)
				}
				if f.activated != 0 || len(f.accepted) != 1 || f.prepared != 1 {
					t.Fatal("unconfirmed or mismatched receipt activated or replaced the deposit")
				}
			})
		}
	}
}

func TestNativeProvisionalRevertMayDisappearOrSucceedWithoutNewDeposit(t *testing.T) {
	f := newAddressFixture(t, true)
	first, err := f.fundNative(t, 100000)
	if err != nil {
		t.Fatal(err)
	}
	f.mine(t, first.TransactionHash, true)
	f.finalized = "0x1"
	waiting, err := f.h.FundAddress(context.Background(), 100000)
	if err != nil || waiting.Phase != "deposit_pending" || waiting.DepositStage != "failed_finalizing" || f.activated != 0 {
		t.Fatalf("provisional failure allowed activation or retry: %+v %v", waiting, err)
	}
	if _, err := f.h.QuoteAddressDeposit(context.Background(), 100000); err == nil {
		t.Fatal("provisional failure allowed a fresh deposit quote")
	}
	delete(f.receipts, first.TransactionHash)
	waiting, err = f.h.FundAddress(context.Background(), 100000)
	if err != nil || waiting.TransactionHash != first.TransactionHash || waiting.Phase != "deposit_pending" {
		t.Fatalf("missing provisional receipt lost recovery: %+v %v", waiting, err)
	}
	f.mine(t, first.TransactionHash, false)
	active, err := f.h.FundAddress(context.Background(), 100000)
	if err != nil || active.Phase != "active" || active.TransactionHash != first.TransactionHash || f.activated != 1 || len(f.accepted) != 1 || f.prepared != 1 {
		t.Fatalf("successful reinclusion did not activate the same saved deposit: %+v %v", active, err)
	}
	for _, raw := range f.submitted {
		if raw != f.submitted[0] {
			t.Fatal("recovery broadcast a replacement transaction")
		}
	}
}

func TestNativeConfirmationValidatesEventsFromTheCanonicalCheckedReceipt(t *testing.T) {
	f := newAddressFixture(t, true)
	first, err := f.fundNative(t, 100000)
	if err != nil {
		t.Fatal(err)
	}
	f.mine(t, first.TransactionHash, false)
	f.finalized = "0x1"
	event := sampleReceipt(depositRecord{Contract: addressTestVault, Commitment: "0x1234", Amount: 100000})
	event.Logs[0].Topics[2] = "0x" + addressABIWord("0x5678")
	f.receipts[first.TransactionHash]["logs"] = event.Logs
	reads := 0
	upstream := f.h.client.inference.Transport
	f.h.client.inference.Transport = withdrawalRoundTripper(func(r *http.Request) (*http.Response, error) {
		raw, err := ioReadAndRestore(r)
		if err != nil {
			return nil, err
		}
		var request struct{ Method string }
		_ = json.Unmarshal(raw, &request)
		response, err := upstream.RoundTrip(r)
		if request.Method == "eth_getTransactionReceipt" {
			reads++
			// A later response could describe a different inclusion. It must
			// not supply unchecked event data for the earlier block check.
			f.mu.Lock()
			f.receipts[first.TransactionHash]["logs"] = sampleReceipt(depositRecord{Contract: addressTestVault, Commitment: "0x1234", Amount: 100000}).Logs
			f.mu.Unlock()
		}
		return response, err
	})
	if _, err := f.h.confirm(context.Background(), first.TransactionHash); err == nil || f.activated != 0 || reads != 1 {
		t.Fatal("activation consumed a second receipt whose event data was not checked for canonical inclusion")
	}
}

func TestNativeReceiptChangeBeforeActivationClearsConfirmedProgress(t *testing.T) {
	for _, change := range []string{"missing", "failed", "block", "event"} {
		t.Run(change, func(t *testing.T) {
			f := newAddressFixture(t, true)
			first, err := f.fundNative(t, 100000)
			if err != nil {
				t.Fatal(err)
			}
			f.mine(t, first.TransactionHash, false)
			f.finalized = "0x1"
			reads := 0
			upstream := f.h.client.inference.Transport
			f.h.client.inference.Transport = withdrawalRoundTripper(func(r *http.Request) (*http.Response, error) {
				raw, err := ioReadAndRestore(r)
				if err != nil {
					return nil, err
				}
				var request struct{ Method string }
				_ = json.Unmarshal(raw, &request)
				if request.Method == "eth_getTransactionReceipt" {
					reads++
					if reads == 2 {
						f.mu.Lock()
						switch change {
						case "missing":
							delete(f.receipts, first.TransactionHash)
						case "failed":
							f.receipts[first.TransactionHash]["status"] = "0x0"
						case "block":
							f.canonical = "0x" + strings.Repeat("b", 64)
						case "event":
							event := sampleReceipt(depositRecord{Contract: addressTestVault, Amount: 100001})
							f.receipts[first.TransactionHash]["logs"] = event.Logs
						}
						f.mu.Unlock()
					}
				}
				return upstream.RoundTrip(r)
			})
			pending, err := f.h.FundAddress(context.Background(), 100000)
			if err == nil || pending.Phase != "deposit_pending" || pending.DepositStage != "" || f.activated != 0 {
				t.Fatalf("changed receipt kept confirmed progress: %+v %v", pending, err)
			}
			f.h = &FundingHandler{client: f.h.client, statePath: f.h.statePath}
			saved, err := f.h.Address(context.Background())
			if err != nil || saved.Phase != "deposit_pending" || saved.TransactionHash != first.TransactionHash {
				t.Fatalf("durable status lost pending recovery: %+v %v", saved, err)
			}
			f.h.client.inference.Transport = upstream
			f.canonical = addressTestBlock
			f.mine(t, first.TransactionHash, false)
			active, err := f.h.FundAddress(context.Background(), 100000)
			if err != nil || active.Phase != "active" || f.activated != 1 || len(f.accepted) != 1 || f.prepared != 1 {
				t.Fatalf("canonical reinclusion changed the saved deposit: %+v %v", active, err)
			}
		})
	}
}

func TestNativeActivationOutageThenReceiptLossClearsConfirmedProgress(t *testing.T) {
	for _, missing := range []bool{false, true} {
		f := newAddressFixture(t, true)
		first, err := f.fundNative(t, 100000)
		if err != nil {
			t.Fatal(err)
		}
		f.mine(t, first.TransactionHash, false)
		f.finalized = "0x1"
		upstream := f.h.client.local.Transport
		f.h.client.local.Transport = withdrawalRoundTripper(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path == "/funding/api/deposit/confirm" {
				return withdrawalJSONResponse(r, 503, map[string]any{"error": "unavailable"}), nil
			}
			return upstream.RoundTrip(r)
		})
		pending, err := f.h.FundAddress(context.Background(), 100000)
		if err == nil || pending.Phase != "confirming" || f.activated != 0 {
			t.Fatalf("activation outage fixture failed: %+v %v", pending, err)
		}
		f.h.client.local.Transport = upstream
		if missing {
			delete(f.receipts, first.TransactionHash)
		} else {
			f.canonical = "0x" + strings.Repeat("b", 64)
		}
		pending, _ = f.h.FundAddress(context.Background(), 100000)
		if pending.Phase != "deposit_pending" || f.activated != 0 || len(f.accepted) != 1 {
			t.Fatal("lost receipt retained confirmed phase or replaced the transaction")
		}
		f.h = &FundingHandler{client: f.h.client, statePath: f.h.statePath}
		saved, err := f.h.Address(context.Background())
		if err != nil || saved.Phase != "deposit_pending" || saved.TransactionHash != first.TransactionHash {
			t.Fatal("restart lost the pending transaction or restored confirmed progress")
		}
	}
}
