# Changelog

## 2026-06-09 - Protocol Crypto and Real Proof Pipeline Hardening

This update implements the crypto and proof-pipeline fixes from `FIX_PLAN.md`.
The runtime path now uses real Stwo/STARK proof artifacts instead of private
witness-envelope replay.

### Issues Addressed

- The Rust proof path did not generate real STARK proofs and relied on local
  witness-envelope replay.
- Request and withdrawal proof envelopes serialized private witness data,
  including `secret_s`, balance blindings, anchors, note metadata, and
  signatures.
- Pedersen balance commitment generators had a public discrete-log relation.
- Server XMSS leaf allocation was not durable and could derive next-state data
  from a predicted signing leaf.
- WOTS secret derivation lacked a dedicated secret-derivation domain tag.
- Felt parsing and public-output encoding were not fully canonical across Rust,
  Cairo, and Solidity.
- Request payload and response payload hashes were not bound tightly enough to
  the exact runtime bytes.
- Client and server root validation did not fully use trusted epoch root
  registry semantics.
- Cairo still had hard-coded Merkle depth assumptions.
- Solidity fact checks needed to match the canonical Cairo/Rust public-output
  hash format.
- Real end-to-end proof coverage was missing for the production request and
  withdrawal branches.

### Changes

#### Real Stwo/STARK Proof Artifacts

- Added `rust/crates/zkapi-proof/src/stwo.rs` with `ScarbStwoProver`.
- Added `rust/crates/zkapi-proof/src/artifact.rs` with opaque
  `ProofArtifact`.
- Added Cairo executable entrypoints in `cairo/src/executables.cairo`:
  - `request_genesis`
  - `request_non_genesis`
  - `request_from_args`
  - `withdrawal_genesis_escape`
  - `withdrawal_non_genesis_escape`
  - `withdrawal_mutual_close`
  - `withdrawal_from_args`
- Registered those executables in `cairo/Scarb.toml`.
- The client default proof mode now invokes `scarb prove` through
  `ScarbStwoProver` and sends `ProofArtifactWire`.
- The server default proof mode now verifies `ProofArtifactWire` via
  `scarb verify --proof-file`.
- The server also extracts public outputs from `proof.json` and requires them
  to hash back to the artifact `public_output_hash`, preventing proof rebinding
  to different public inputs.
- Private witness-envelope replay remains available only behind the explicit
  `dev-witness-envelope` feature.

#### Proof Coverage

- Added real Stwo proof tests for:
  - non-genesis request proof generation and verification;
  - mutual-close withdrawal proof generation and verification;
  - request rejection for a bad state signature root;
  - request rejection for insufficient balance;
  - request rejection for a wrong Merkle path/root;
  - withdrawal rejection for a bad clearance signature root;
  - public-output hash mismatch when the withdrawal destination is changed;
  - artifact rebinding rejection.
- Added a server runtime test that processes a real Stwo request artifact, then
  verifies recovery output and archived challenge evidence.
- Added `scripts/prove_genesis_stwo.sh` to prove and verify all fixture Cairo
  executable branches through Scarb/Stwo.

#### Pedersen Commitment Binding

- Replaced the old public-scalar-multiple generator derivation with committed
  independent Stark-curve generator constants.
- Updated both Rust and Cairo Pedersen implementations.
- Added a reproducible generator script under `scripts/`.
- Added regression coverage for the old reopening issue and parity checks for
  the new constants.

#### XMSS/WOTS Signing Safety

- Added durable signer state for state and clearance XMSS trees.
- Added an atomic `sign_next_state` flow that reserves the exact leaf used to
  derive blind delta, anchor, state message, and signature.
- Added crash, restart, and concurrency tests so reserved leaves are not reused.
- Enforced production XMSS auth-path height while keeping smaller trees limited
  to tests.
- Added `DOMAIN_XMSS_SK` for domain-separated WOTS secret derivation.

#### Canonical Types and Public Outputs

- Added checked `Felt252` parsing for canonical `< STARK_FIELD_PRIME` values.
- Centralized request and withdrawal public-output encoding in Rust.
- Added canonical public-output hash helpers, including helpers that hash Cairo
  proof outputs extracted from Stwo proof JSON.
- Updated wire types to carry `ProofArtifactWire` instead of proof envelopes in
  production.
- Added stable cross-language vectors for Rust/Solidity public-output hashes.

#### Runtime Request/Response Binding

- The server recomputes `payload_hash` from the actual request payload bytes
  and rejects mismatches.
- The server computes `response_hash` from the actual response payload returned
  to the client.
- The client verifies response payload/hash pairs on normal request and
  recovery paths.
- Recovery journal and transcript paths keep the exact payload hash and
  response hash needed for idempotent recovery.

#### Trusted Roots and Lifecycle Support

- Added trusted epoch root lookup semantics for client and server checks.
- The server accepts valid prior trusted state roots where the protocol allows
  it.
- The client rejects response and clearance signatures under unpublished roots.
- The challenge watcher now archives and uses real proof artifacts rather than
  private witness envelopes.

#### Solidity Fact Adapter

- Updated `FactRegistryAdapter` to use canonical request/withdrawal
  public-output hashing.
- Added `contracts/test/FactRegistryAdapter.t.sol` coverage for request facts,
  withdrawal facts, wrong statement types, wrong program hash, and modified
  public inputs.

#### Cairo Cleanup

- Replaced hard-coded Merkle depth loops with `MERKLE_DEPTH`.
- Added `scripts/check_merkle_depth_constants.sh` to catch reintroduced
  `while i < 32` style constants in Merkle/proof path code.
- Updated Cairo Pedersen balance commitment code to match the new Rust
  generator constants.

#### Documentation

- Added `docs/src/stwo.md`.
- Updated docs to describe proof artifacts, real Stwo/Scarb bridge behavior,
  runtime verification, testing coverage, and the dev-only witness-envelope
  escape hatch.

### Validation

The following checks passed after the changes:

- `cargo test --manifest-path rust/Cargo.toml --workspace`
- `cargo test --manifest-path rust/Cargo.toml --workspace --all-features`
- `cargo test --manifest-path rust/Cargo.toml -p zkapi-proof --no-default-features`
- `cargo test --manifest-path rust/Cargo.toml -p zkapi-server --no-default-features`
- `scarb test`
- `forge test --offline`
- `scripts/check_merkle_depth_constants.sh`
- `scripts/prove_genesis_stwo.sh`
- `cargo fmt --manifest-path rust/Cargo.toml --all`
- `scarb fmt`
- `git diff --check`

Note: local `scarb` and `forge` runs print
`(eval):5: parse error near 'end'` from the shell init environment, but the
commands exit successfully.

### Remaining Engineering Note

The real proof pipeline is implemented through Scarb CLI orchestration:
`scarb prove` generates the Stwo proof and `scarb verify` verifies it. This is
real STARK generation and verification, not mock replay. A future optimization
can replace the CLI bridge with a direct Rust integration around
`stwo-vm-runner` and `stwo-cairo-prover` if deployment latency or process
management requires it.
