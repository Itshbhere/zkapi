# Operating the v2 escape challenge service

`zkapi-challenged` is the repository's event-to-submission implementation. It
reads the active server's SQLite transcript and lease database, polls
`EscapeWithdrawalInitiated`, reconstructs the archived v2 request and its
256-byte Groth16 proof, obtains a current zero-slot Merkle path, simulates the
vault call, and submits `challengeEscapeWithdrawal`.

Build and run it separately from the API server:

```sh
cargo build --release -p zkapi-serverd --bin zkapi-challenged
RUST_LOG=info target/release/zkapi-challenged \
  --rpc-url "$OPERATOR_SIGNER_RPC_URL" \
  --indexer-url http://127.0.0.1:3001 \
  --sender "$DEDICATED_CHALLENGE_ACCOUNT" \
  --chain-id "$CHAIN_ID" \
  --contract-address "$VAULT_ADDRESS" \
  --from-block "$VAULT_DEPLOYMENT_BLOCK" \
  --db-path /var/lib/zkapi/server.db \
  --checkpoint /var/lib/zkapi/challenges.json \
  --proof-setup-dir /var/lib/zkapi/setup/v2 \
  --confirmations 2
```

All deployment and signer settings are explicit. The RPC must implement
`eth_sendTransaction` using an operator-controlled signer for the dedicated,
funded sender address. A public read-only RPC is insufficient. Configure signing
permissions at that RPC; do not expose an unlocked account over a public RPC.
Use one service and one exclusive sender per deployment. The daemon locks its
checkpoint file to prevent two instances from sharing it. Keep the database,
checkpoint, and signer available throughout every challenge window.

For the AWS deployment, [the bounded signer sidecar](../docker/aws/signer/README.md)
provides that signer RPC on a private container network. It requires an explicit
`ZKAPI_CHALLENGE_CHAIN_ID` of 1 (Mainnet) or 11155111 (Sepolia), with no default,
and accepts only zero-value challenge calls for that chain and vault,
checks the full ABI and live simulation, and preserves the daemon's reserved
nonce. Its dedicated private key belongs only in the signer's environment.
Keep its port unpublished; the public API reverse proxy must never route to it.

The server, browser/client proving keys, verifier, and daemon must all use the
same deployment and circuit setup. This revision's circuit identifier is
`zkapi-v2-note-bound-v1`. The key loader rejects old unversioned setup files.
Replacing a local key directory does not upgrade an existing deployed verifier.
A vault also needs the historical-root challenge fix: the archived request root
is intentionally preserved while the restoration path uses the current root.

## Recovery and verification

The checkpoint records the deployment and sender, next scan block, block hash, pending
nullifiers, reserved transaction nonces, and returned transaction hashes. It is
written before advancing to submission, with atomic replacement after syncing
the file. The service refuses checkpoints from another chain, vault or starting
block. Scans are bounded to 1,000 blocks. A changed checkpoint block hash causes
replay from the deployment block; pending entries are reconciled against the
current vault state before submission.

Each poll verifies the RPC chain ID. Before sending, the daemon checks the
on-chain pending nullifier, deadline, current-root zero-slot proof, archived
proof/deployment binding, and the complete call with `eth_call`. It does not
rewrite a historical public root. Stale indexer paths and RPC/signer failures
leave work queued for retry. A finalized transcript or an active issued lease supplies challenge evidence.
An issued lease uses its exact durable request proof, after the complete request
binding is matched to the reservation, so an unavailable station usage receipt
cannot let a stale escape mature. Mere provisioning is insufficient: the service
keeps that request pending until a key has been durably activated or usage
finalized. Evidence reconstruction is read-only and never signs a balance or
changes usage accounting. Returned
transaction hashes suppress duplicate submissions until a receipt is available;
reverted calls refresh their path and retry. Dropped or orphaned transactions
are resubmitted with their reserved nonce once the RPC no longer knows their hash. Ambiguous send errors retain the
nonce across restart. Successful challenges are retired only after the pending withdrawal also
disappears at the configured confirmation depth, preventing a tip reorg from
silently losing the obligation.

Watch `escape challenge requires retry`, `challenge poll failed`, and especially
`MISSED escape challenge deadline` logs. This service does not configure
monitoring, gas funding, fee replacement, redundant infrastructure or external
signer availability. A transaction stuck without a receipt needs operator
intervention at the signer. RPC, indexer and signing latency, confirmation depth,
and any restart/replay time must fit within the vault's challenge window.

Regression tests use a real v2 proof accepted and archived by the active server
processor, plus a local JSON-RPC/indexer mock to exercise event polling,
submission failure, restart, same-nonce retry, reverted receipts, and confirmed
on-chain completion. This is
not evidence that a live deployment has the daemon configured or running.
