package zkapi

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"
)

// All I/O stays within the synctest bubble, so these tests exercise the real
// sixty-second timers without sleeps on the wall clock or scheduler tolerances.
type automaticSettlementFixture struct {
	client      *Client
	pending     atomic.Bool
	leases      atomic.Int32
	issued      atomic.Int32
	settlements atomic.Int32
	inferences  atomic.Int32
	expires     time.Duration
	settle      func(*http.Request) (*http.Response, error)
}

func automaticSettlementResponse(r *http.Request, status int, body string) *http.Response {
	return &http.Response{StatusCode: status, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(body)), Request: r}
}

func newAutomaticSettlementFixture(t *testing.T, window time.Duration) *automaticSettlementFixture {
	t.Helper()
	f := &automaticSettlementFixture{expires: time.Hour}
	client, err := New(Config{
		ClientURL: "http://127.0.0.1:43134", BridgeToken: testBridgeToken,
		InferenceBaseURL: "https://provider.invalid/api/v1", KeyReuseWindow: window,
		HTTPClient: &http.Client{Transport: budgetTransport(func(r *http.Request) (*http.Response, error) {
			f.inferences.Add(1)
			return automaticSettlementResponse(r, http.StatusOK, `{"choices":[]}`), nil
		})},
	})
	if err != nil {
		t.Fatal(err)
	}
	f.client = client
	client.settlementPoll = time.Second
	client.local.Transport = budgetTransport(func(r *http.Request) (*http.Response, error) {
		if r.Header.Get("Authorization") != "Bearer "+testBridgeToken {
			t.Error("automatic settlement did not authenticate to the local wallet")
		}
		switch r.URL.Path {
		case "/oa/v1/status":
			body, _ := json.Marshal(testPolicy("mainnet"))
			return automaticSettlementResponse(r, http.StatusOK, string(body)), nil
		case "/oa/v1/lease":
			f.leases.Add(1)
			if f.pending.Load() {
				return automaticSettlementResponse(r, http.StatusConflict, `{"error":{"code":"lease_pending"}}`), nil
			}
			f.pending.Store(true)
			body, _ := json.Marshal(map[string]any{
				"api_key":  fmt.Sprintf("automatic-settlement-key-%d", f.issued.Add(1)),
				"base_url": client.config.InferenceBaseURL, "expires_at": time.Now().Add(f.expires).Unix(),
				"verified": true, "verification_status": "verified",
			})
			return automaticSettlementResponse(r, http.StatusOK, string(body)), nil
		case "/wallet/settle":
			f.settlements.Add(1)
			body, _ := io.ReadAll(r.Body)
			if r.Method != http.MethodPost || len(body) != 0 || r.Header.Get("Origin") != "" {
				t.Error("automatic settlement must use a prompt-free local POST")
			}
			if f.settle != nil {
				return f.settle(r)
			}
			f.pending.Store(false)
			return automaticSettlementResponse(r, http.StatusOK, `{"pending_request":false}`), nil
		default:
			t.Errorf("unexpected local endpoint %s", r.URL.Path)
			return automaticSettlementResponse(r, http.StatusNotFound, `{}`), nil
		}
	})
	addTestModelPolicy(client, map[string]uint64{"example/model": 1, "premium/model": 3}, nil)
	return f
}

func startAutomaticSettlement(t *testing.T, client *Client) (context.CancelFunc, <-chan struct{}) {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		client.RunSettlement(ctx)
		close(done)
	}()
	t.Cleanup(cancel)
	return cancel, done
}

func TestAutomaticSettlementEndsFixedGroupWithoutAnotherRequest(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newAutomaticSettlementFixture(t, time.Minute)
		cancel, _ := startAutomaticSettlement(t, f.client)
		defer cancel()
		drainReuseResponse(t, f.client, context.Background(), queuedTestPrompt)
		time.Sleep(45 * time.Second)
		drainReuseResponse(t, f.client, context.Background(), queuedTestPrompt)
		if f.issued.Load() != 1 {
			t.Fatal("request inside the group did not reuse its key")
		}
		time.Sleep(15*time.Second - time.Nanosecond)
		synctest.Wait()
		if f.settlements.Load() != 0 {
			t.Fatal("settlement ran before the fixed group deadline")
		}
		time.Sleep(time.Nanosecond)
		synctest.Wait()
		if f.settlements.Load() != 1 || f.inferences.Load() != 2 || f.leases.Load() != 1 {
			t.Fatalf("group did not settle at its original deadline: settlements=%d inference=%d leases=%d", f.settlements.Load(), f.inferences.Load(), f.leases.Load())
		}
		time.Sleep(5 * time.Minute)
		synctest.Wait()
		if f.settlements.Load() != 1 || f.leases.Load() != 1 {
			t.Fatal("an idle settled group repeated settlement or acquired another key")
		}
	})
}

func TestAutomaticSettlementHonorsEarlierProviderExpiry(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newAutomaticSettlementFixture(t, time.Minute)
		f.expires = 10 * time.Second
		cancel, _ := startAutomaticSettlement(t, f.client)
		defer cancel()
		drainReuseResponse(t, f.client, context.Background(), queuedTestPrompt)
		time.Sleep(9*time.Second - time.Nanosecond)
		synctest.Wait()
		if f.settlements.Load() != 0 {
			t.Fatal("settled before the provider expiry guard")
		}
		time.Sleep(time.Nanosecond)
		synctest.Wait()
		if f.settlements.Load() != 1 {
			t.Fatal("provider expiry did not shorten the group deadline")
		}
	})
}

func TestAutomaticSettlementWaitsForResponseOwnership(t *testing.T) {
	for _, window := range []time.Duration{time.Minute, 0} {
		for _, finish := range []string{"drain", "close", "cancel"} {
			t.Run(fmt.Sprintf("window_%s/%s", window, finish), func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					f := newAutomaticSettlementFixture(t, window)
					stop, _ := startAutomaticSettlement(t, f.client)
					defer stop()
					requestCtx, cancelRequest := context.WithCancel(context.Background())
					defer cancelRequest()
					response, err := f.client.Complete(requestCtx, queuedTestPrompt)
					if err != nil {
						t.Fatal(err)
					}
					defer response.Body.Close()
					time.Sleep(window + 5*time.Second)
					synctest.Wait()
					if f.settlements.Load() != 0 {
						t.Fatal("settlement retired a key still owned by its provider response")
					}
					switch finish {
					case "drain":
						if _, err := io.Copy(io.Discard, response.Body); err != nil {
							t.Fatal(err)
						}
					case "close":
						if err := response.Body.Close(); err != nil {
							t.Fatal(err)
						}
					case "cancel":
						cancelRequest()
					}
					synctest.Wait()
					if f.settlements.Load() != 1 {
						t.Fatal("overdue group did not settle immediately when its response released the key")
					}
				})
			})
		}
	}
}

func TestAutomaticSettlementDoesNotRetireReplacementAtOldDeadline(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newAutomaticSettlementFixture(t, time.Minute)
		cancel, _ := startAutomaticSettlement(t, f.client)
		defer cancel()
		drainReuseResponse(t, f.client, context.Background(), queuedTestPrompt)
		time.Sleep(20 * time.Second)
		// A different spending cap retires the first key early. Its old timer
		// must never retire the replacement, whose deadline is twenty seconds later.
		drainReuseResponse(t, f.client, context.Background(), json.RawMessage(`{"model":"premium/model","messages":[]}`))
		if f.issued.Load() != 2 || f.settlements.Load() != 1 {
			t.Fatal("changed budget did not retire and replace the first key")
		}
		time.Sleep(40 * time.Second)
		synctest.Wait()
		if f.settlements.Load() != 1 {
			t.Fatal("stale timer retired the replacement group before its own deadline")
		}
		time.Sleep(20 * time.Second)
		synctest.Wait()
		if f.settlements.Load() != 2 {
			t.Fatal("replacement group did not settle at its own deadline")
		}
	})
}

func TestAutomaticSettlementSharesRetirementWithQueuedRequest(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newAutomaticSettlementFixture(t, time.Minute)
		cancel, _ := startAutomaticSettlement(t, f.client)
		defer cancel()
		first, err := f.client.Complete(context.Background(), queuedTestPrompt)
		if err != nil {
			t.Fatal(err)
		}
		defer first.Body.Close()
		// Queue the next request before the timer becomes due. Both paths will
		// want to retire the expired group when its response finally releases it.
		result := make(chan error, 1)
		go func() {
			response, err := f.client.Complete(context.Background(), queuedTestPrompt)
			if err == nil {
				_, err = io.Copy(io.Discard, response.Body)
				_ = response.Body.Close()
			}
			result <- err
		}()
		synctest.Wait()
		time.Sleep(time.Minute)
		synctest.Wait()
		if f.settlements.Load() != 0 || f.issued.Load() != 1 {
			t.Fatal("queued retirement bypassed the live response")
		}
		_, _ = io.Copy(io.Discard, first.Body)
		synctest.Wait()
		select {
		case err := <-result:
			if err != nil {
				t.Fatal(err)
			}
		default:
			t.Fatal("queued request did not resume after retirement")
		}
		if f.settlements.Load() != 1 || f.issued.Load() != 2 || f.inferences.Load() != 2 {
			t.Fatalf("timer/request race retired twice: settlements=%d issued=%d inference=%d", f.settlements.Load(), f.issued.Load(), f.inferences.Load())
		}
	})
}

func TestAutomaticSettlementCancellationStopsPendingWork(t *testing.T) {
	for _, state := range []string{"waiting_deadline", "waiting_response", "in_flight"} {
		t.Run(state, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				f := newAutomaticSettlementFixture(t, time.Minute)
				if state == "in_flight" {
					f.settle = func(r *http.Request) (*http.Response, error) {
						<-r.Context().Done()
						return nil, r.Context().Err()
					}
				}
				cancel, done := startAutomaticSettlement(t, f.client)
				defer cancel()
				response, err := f.client.Complete(context.Background(), queuedTestPrompt)
				if err != nil {
					t.Fatal(err)
				}
				defer response.Body.Close()
				if state != "waiting_response" {
					_, _ = io.Copy(io.Discard, response.Body)
				}
				if state != "waiting_deadline" {
					time.Sleep(time.Minute)
					synctest.Wait()
				}
				cancel()
				synctest.Wait()
				select {
				case <-done:
				default:
					t.Fatal("daemon cancellation did not stop its settlement worker")
				}
				_ = response.Body.Close()
				time.Sleep(2 * time.Minute)
				synctest.Wait()
				want := int32(0)
				if state == "in_flight" {
					want = 1
				}
				if f.settlements.Load() != want {
					t.Fatalf("canceled worker started another RPC: got=%d want=%d", f.settlements.Load(), want)
				}
			})
		})
	}
}

func TestAutomaticSettlementDoesNotRepeatUncertainRetirement(t *testing.T) {
	for _, outcome := range []string{"pending_conflict", "pending_body", "invalid", "lost"} {
		t.Run(outcome, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				f := newAutomaticSettlementFixture(t, time.Minute)
				f.settle = func(r *http.Request) (*http.Response, error) {
					switch outcome {
					case "pending_conflict":
						return automaticSettlementResponse(r, http.StatusConflict, `{"error":{"code":"pending_request"}}`), nil
					case "pending_body":
						return automaticSettlementResponse(r, http.StatusOK, `{"pending_request":true}`), nil
					case "invalid":
						return automaticSettlementResponse(r, http.StatusOK, `{}`), nil
					default:
						return nil, errors.New("simulated lost settlement reply")
					}
				}
				cancel, _ := startAutomaticSettlement(t, f.client)
				defer cancel()
				drainReuseResponse(t, f.client, context.Background(), queuedTestPrompt)
				time.Sleep(3 * time.Minute)
				synctest.Wait()
				if f.settlements.Load() != 1 {
					t.Fatal("uncertain background settlement was repeated")
				}
				for range 2 {
					ctx, stop := context.WithTimeout(context.Background(), 3*time.Second)
					response, err := f.client.Complete(ctx, queuedTestPrompt)
					stop()
					if response != nil {
						_ = response.Body.Close()
					}
					if err == nil {
						t.Fatal("pending wallet unexpectedly allowed another inference")
					}
				}
				if f.settlements.Load() != 1 || f.inferences.Load() != 1 {
					t.Fatal("later requests repeated the uncertain retirement or inference")
				}
				// A later successful issuance remains authoritative: signed wallet
				// recovery can complete without another retirement RPC from this client.
				f.pending.Store(false)
				drainReuseResponse(t, f.client, context.Background(), queuedTestPrompt)
				if f.issued.Load() != 2 || f.settlements.Load() != 1 || f.inferences.Load() != 2 {
					t.Fatal("saved retirement result blocked recovered wallet issuance")
				}
			})
		})
	}
}
