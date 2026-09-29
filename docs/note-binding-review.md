# Note-bound circuit revision

> Historical repair/validation record. The subsequent [native-only cleanup](native-only-cleanup.md)
> removes legacy paths; the revision and deployment evidence below remains historical.


The September 22 review demonstrated that a valid server signature on a balance
could be transplanted between two active notes. The original commitment only
bound balance and randomness. Membership, nullifier and signature verification
were individually valid but did not refer to the same note.

The candidate repair uses:

```
L = Poseidon(leaf_domain, note_id, registration(secret), deposit, expiry)
C = balance * G + blinding * H + L * J
```

Both request and withdrawal circuits compute `L` from the exact private values
used by the Merkle-membership proof. They use that same `L` to open the signed
commitment. `J` is independently derived with Keccak try-and-increment from
`zkapi.v2.note-binding.generator.v1`, choosing even y and clearing the cofactor.
No known scalar relation to `G` or `H` is introduced. The full canonical BN254
field representation of `L` is multiplied by J, naturally reducing modulo the
Baby-JubJub subgroup order, in both native code and the circuit.

The server still updates an anonymous commitment by subtracting `charge * G`
and adding `blind_delta * H`. The note component survives that update without
revealing the note leaf. A fresh uniformly random blinding masks the leaf in
public commitments. Changing the note while retaining the signature requires
breaking the commitment binding/signature or finding an appropriate hash
collision; balancing the deposit amounts alone cannot do it.

This is a proposed protocol repair, not a cryptographic audit or a replacement
setup ceremony. It adds the discrete-log binding assumption for the independent
third generator, alongside the existing Poseidon, Schnorr and Groth16
assumptions. Groth16/BN254 and Baby-JubJub are not post-quantum.

## Compatibility and setup

The JSON v2 schemas and public-input order stay the same. The circuit revision
is `zkapi-v2-note-bound-v1`. Proving and verifying key files carry that header;
loaders reject legacy files before decoding. Public deployment manifests must
advertise `proof_setup.circuit_id` and clients must pin the new key hashes and
vault address. The header is an accidental-mismatch guard, not setup provenance
or a substitute for independently pinned key hashes.

The checked-in `protocol/setup/v2` directory now contains a **new single-party
development setup** produced with OS randomness by the repaired source. The
historical directory name does not imply compatibility with the old keys.
The matching Solidity verifier and SDK WASM/proving keys must travel together.
These test artifacts do not establish a reviewed multiparty ceremony, and are
not production launch artifacts. A production release needs an independently
reviewed design and setup with recorded provenance and trust assumptions.

Existing vaults have immutable verifiers and keys. They cannot be repaired by
replacing a server or frontend. Existing unbound signed states are incompatible
with this circuit. Preserve old wallet state and use the corresponding legacy
client for recovery/withdrawal; do not rewrite its commitment or label an old
vault as this revision. A fresh reviewed deployment, revised client pins, and
an explicit fund migration are required. The [September 22 ERC-20 deployment](deployments/sepolia-note-bound-20260922.md)
records the review branch's native-client lifecycle, historical-root challenge
and OA direct-mode acceptance. This merged branch preserves the newer native
ETH Sepolia vault and its independent pins; see [native ETH billing](native-eth-billing.md).
Both deployments use the same repaired circuit/setup. Their vaults, denomination,
signing keys and wallet state are separate and must never be interchanged.
The Mainnet configuration pins independent native ETH contracts verified at
finalized chain state and removes that configuration's migration guard.
The backend and separate native Mainnet application are published. See the
[Mainnet rollout record](deployments/mainnet-native-eth-20260928.md).
The generic migration guard remains available and tested: an old manifest
cannot be relabeled to enable funding against a guarded vault. Legacy USDC
recovery continues through its original deployment and client.

## Regression boundary

Circuit tests reject transplanted request and withdrawal signatures for equal
and unequal deposits, including cases where the balance is less than both
deposits. The active-server regression places A and B in one tree, obtains
A's genuine signed 99-unit state from a real proof, continues A to 98, and
rejects a B witness carrying that 99-unit state. Honest escape and mutual
withdrawal proofs still verify. Contract tests separately preserve challenges
across unrelated deposits/closes, and the challenge consumer test reads the
exact v2 transcript written by the active processor.

These local tests do not prove a live OA Chat happy path, availability of its
provider/login services, or deployment identity. The host application owns
funding screens, token-purchase feedback and OAuth; it is not included here.

## Supported modes and operational follow-up

The browser's packaged configs require OA-org key sourcing. The native CLI
still defaults to ordinary proxy mode and can opt into direct OpenRouter leases;
the server exposes direct issuance when configured with either an OpenRouter
management key or an OA org URL and dedicated service credential.
All three modes remain supported in source; this does not establish which are
enabled in a live deployment.

The repository now includes `zkapi-challenged`, a runnable event-to-submission
service with durable retries and explicit signer configuration. It must be
operated alongside serverd and indexerd. The fresh Sepolia deployment runs it
with a restricted signer sidecar and has completed a live historical-root
challenge. See [service operation](challenge-service.md).

Concurrent ordinary-proxy retries coordinate within a shared store instance.
Process crashes, cancellation after upstream acceptance, and separate writer
processes still need upstream idempotency or deployment-level coordination.
Direct OpenRouter retirement persists disable, grace, usage capture and
revocation stages and does not sign before confirmed deletion. Its aggregate
usage API supplies no authoritative final receipt: the configured grace must
cover in-flight calls and accounting delay. OA-org finalized receipts provide
a separate path. Audit/revoke legacy keys whose old records were already marked
finalized; the repair cannot undo previously issued unbound signed balances.

## Reproducing validation

From the integration repository root:

```sh
cargo test --release --workspace
cargo test --release --manifest-path protocol/rust/Cargo.toml --workspace
cargo run --release --manifest-path protocol/rust/Cargo.toml -p zkapi-proof --example wasm_roundtrip -- "$PWD/scripts/prove-browser-fixture.mjs" "$PWD/sdk"
(cd protocol/contracts && forge test)
npm ci
npm test
npm run build:browser
npm run build:mainnet
npm run verify:assets
```

The committed EVM fixture can be regenerated with the protocol
`vault_challenge_fixture` example against `protocol/setup/v2`. It exercises a
real signed-state request followed by an unrelated deposit or mutual close,
an escape proof at the changed root, and successful challenge at the real
Groth16 adapter. A separate test rejects a rewritten archived request root.
The WASM command generates proofs using the packaged JS/WASM/proving-key bytes
and verifies them against the native keys; it requires Node 24 or later.

The original protocol repair is commit `49164f6`, published on
`codex/zkapi-review-fixes-sepolia` in `mingyech/zkapi`. The merged native integration pinned protocol `8b2d4e3`, whose circuit, setup,
WASM sources and challenge regressions are identical to `49164f6`, with native
ETH contract support added. The native challenge path also preserves active
issued-lease evidence during usage-receipt outages and full deployment/request
binding for replay. Native ETH rejects ordinary proxy billing; the ERC-20 proxy
and direct OpenRouter optional-mode fixes are retained for their supported deployments. The fresh Sepolia deployment is recorded separately
in the [deployment notes](deployments/sepolia-note-bound-20260922.md); existing
vaults are not upgraded by this source revision.

The protocol source is now tracked directly under `protocol/` in this
repository, imported from `8b2d4e3da921f956e1eb6b93afbf722a877c060c`.
The revisions above record the source provenance of the earlier validation
and deployments; no submodule checkout is required.

Verified locally on September 22, 2026: 82 protocol Rust tests, 124 integration
Rust tests, 189 SDK tests, and 20 Solidity tests passed. The shipped-WASM proof
round trip passed for request and escape withdrawal; the final source rebuild
produced byte-identical WASM to that tested bundle. Both network SDK builds,
asset hashes, `cargo fmt --check`, root all-target Clippy and protocol workspace
Clippy passed. Those automated tests used local provider/RPC mocks. Subsequent
native acceptance against the live Sepolia deployment covered provider usage,
OA verification and settlement, withdrawals, and historical-root challenges
as recorded in the deployment notes. Those earlier acceptance runs did not cover OA Chat UI/login observations.
Current host application verification is tracked separately in oa-chat docs.

## Native merge acceptance, 2026-09-27

The native branch merges review commit `2eda8f3` with the existing `6f12f3b`
line. That merge retained protocol `8b2d4e3` for native ETH compatibility; no new
trusted setup or deployed-vault migration is introduced by this merge.

The merge additionally preserves escape challenge evidence for direct leases
in `retiring`, `disabled` and `revoking` states, with an active-processor
real-proof regression. Proxy concurrency is tested with 16 real-proof retries
across two processors sharing one store. Browser deposit confirmation now
separates the durable exact-operation commit from later wallet-status refresh;
a display refresh failure must not report an already committed deposit as
failed, while proof/indexer/storage failures before commit remain errors.

Fresh acceptance includes 82 protocol Rust tests, 141 integration Rust tests,
27 Solidity tests, two shipped-WASM proofs verified natively, 11 restricted
signer tests, and four no-network launcher smoke checks. Rust formatting and
integration Clippy pass. The demo deployment script compiles. Browser SDK test
and network-package build results are recorded with the host app's immutable
release pin. The host tracks manual-review observations and publication status
separately; passing source tests are not a claim of a new live acceptance run.
