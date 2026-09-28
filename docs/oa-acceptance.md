# OA-org Sepolia acceptance

Run this separately after `scripts/accept-sepolia.py --challenge` succeeds.
It sends four Sepolia transactions (mint, approve, deposit, mutual close) and
one paid completion capped at eight output tokens. The acceptance account must
have Sepolia ETH and differ from the operator treasury and challenge sender.

```sh
python3 scripts/accept-sepolia-oa.py \
  --deployment https://d24dltwirql2l5.cloudfront.net/config.json \
  --zkapi ./target/release/zkapi \
  --keystore /private/acceptance-keystore.json \
  --password-file /private/acceptance-password \
  --run-dir /private/new-oa-acceptance-run \
  --model "$PAID_MODEL"
```

The run directory must be new and is created with private permissions. Use a
paid OpenRouter model supporting `max_tokens: 8`; a zero measured charge fails
the billing assertion. `--settlement explicit` is the default and requires
receipt evidence that the key closed before its provider expiry. A separate run
with `--settlement expiry` leaves the child key active until the server's
five-minute expiry/settlement cycle completes. Neither mode alters the proxy
or historical-root challenge acceptance script.

The helper launches the native daemon with these trust settings before `client`:

```sh
zkapi --require-oa-org-key-source \
  --oa-verifier-url https://verifier2.openanonymity.ai \
  --openrouter-inference-base https://openrouter.ai/api/v1 \
  client --mode direct-openrouter --deployment /private/run/deployment.json \
  --state-dir /private/run/wallet-oa --listen 127.0.0.1:11434 --no-fund
```

`--require-oa-org-key-source` rejects a fallback to ordinary OpenRouter keys.
Before inference, the native client checks the issuer's verifier URL against
the independently configured URL and posts the station/org key evidence to
`/submit_key`. It sends the completion only after that verifier returns
`status: verified`. The helper records the native success log and requires an
actual provider completion. Runtime API keys remain inside the native process
and never enter helper output or saved lease metadata.

The local API sequence is:

1. Fund a note through `/deposit/prepare` and `/deposit/confirm` using the
   existing acceptance transaction helpers.
2. `POST /request` with a normal `/v1/chat/completions` request and
   `max_tokens: 8`. In direct mode the native client proves a prompt-free lease
   authorization, obtains/verifies its OA key, then sends the prompt directly
   to the pinned OpenRouter origin. The immediate `charge_applied: 0` means
   usage has not settled yet.
3. Inspect `/zkapi/v1/config` and `/wallet/status` for the active lease and
   pending wallet request. Public `GET /v2/openrouter/leases/{id}` returns only
   non-secret status/timing metadata.
4. In explicit mode, `POST /wallet/settle` with `{}`. The daemon authenticates
   retirement with the original proof and retries pending receipts internally
   for up to 45 seconds. The helper retries pending settlement for up to 600
   seconds and rejects a result that only closed at natural expiry. In expiry
   mode, the helper waits for automatic server settlement
   and the native client's five-second recovery loop instead.
5. Require a finalized OA usage receipt through `/v2/requests/{id}`, a nonzero
   charge within the proof bound, and the same deducted balance in the native
   wallet. The native wallet verifies the server's new signed state before
   clearing its pending request. The helper checks receipt lifecycle fields
   and presence of station/org evidence, then verifies the mutual-close refund
   and exact treasury credit on chain.

`summary.json` records measured charge, completion tokens, settlement mode,
whether the receipt reports closure before provider expiry, and transaction
hashes. Private receipt and wallet files remain in the run directory on both
success and failure. If a stage fails, preserve that directory for recovery;
do not replace the wallet or redeposit merely to retry a pending lease.
