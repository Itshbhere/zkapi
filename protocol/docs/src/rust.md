# Rust Services and SDK

Rust provides the operational state machines around the protocol.

## Shared Types

- canonical field/wire types: [`rust/crates/zkapi-types`](../../rust/crates/zkapi-types)
- domain tags and public inputs live here so Cairo, Solidity, and Rust agree on shape and ordering

## Core Helpers

- Poseidon helpers: [`rust/crates/zkapi-core/src/poseidon.rs`](../../rust/crates/zkapi-core/src/poseidon.rs)
- note leaf and registration commitment: [`rust/crates/zkapi-core/src/leaf.rs`](../../rust/crates/zkapi-core/src/leaf.rs)
- Merkle tree mirror: [`rust/crates/zkapi-core/src/merkle.rs`](../../rust/crates/zkapi-core/src/merkle.rs)
- nullifiers and state-message helpers: [`rust/crates/zkapi-core/src/nullifier.rs`](../../rust/crates/zkapi-core/src/nullifier.rs), [`rust/crates/zkapi-core/src/commitment.rs`](../../rust/crates/zkapi-core/src/commitment.rs)

## Crypto

- Pedersen commitment: [`rust/crates/zkapi-crypto/src/pedersen.rs`](../../rust/crates/zkapi-crypto/src/pedersen.rs)
- WOTS+: [`rust/crates/zkapi-crypto/src/wots.rs`](../../rust/crates/zkapi-crypto/src/wots.rs)
- XMSS: [`rust/crates/zkapi-crypto/src/xmss.rs`](../../rust/crates/zkapi-crypto/src/xmss.rs)

## Proof Artifact Layer

The Rust client/server path now uses opaque proof artifacts by default:

- request witness builder and Cairo args: [`rust/crates/zkapi-proof/src/request.rs`](../../rust/crates/zkapi-proof/src/request.rs)
- withdrawal witness builder and Cairo args: [`rust/crates/zkapi-proof/src/withdrawal.rs`](../../rust/crates/zkapi-proof/src/withdrawal.rs)
- Stwo/Scarb bridge: [`rust/crates/zkapi-proof/src/stwo.rs`](../../rust/crates/zkapi-proof/src/stwo.rs)

What this means operationally:

- the client serializes private witness data only into local Cairo executable arguments
- Scarb proves and verifies the Cairo execution before the client sends a request
- the wire request carries an opaque proof artifact plus canonical public-output hash
- the server rejects mismatched public-output hashes and runs `scarb verify --proof-file`

The old witness envelope/local replay path is available only behind the
`dev-witness-envelope` feature for development tests.

## Client SDK

The wallet state machine lives in [`rust/crates/zkapi-client/src/wallet.rs`](../../rust/crates/zkapi-client/src/wallet.rs).

Important behaviors:

- it persists `state_sig_epoch`, `state_sig_root`, and the server signature
- it stores `user_rerandomization` in the write-ahead journal
- it verifies XMSS signatures on server responses and clearance responses
- it can recover the exact next blinding after a crash

The durable state model itself lives in:

- note state: [`rust/crates/zkapi-client/src/note_state.rs`](../../rust/crates/zkapi-client/src/note_state.rs)
- journal: [`rust/crates/zkapi-client/src/journal.rs`](../../rust/crates/zkapi-client/src/journal.rs)

## Server

The server core now consists of:

- request processor: [`rust/crates/zkapi-server/src/processor.rs`](../../rust/crates/zkapi-server/src/processor.rs)
- provider boundary: [`rust/crates/zkapi-server/src/provider.rs`](../../rust/crates/zkapi-server/src/provider.rs)
- nullifier/transcript DB: [`rust/crates/zkapi-server/src/nullifier_store.rs`](../../rust/crates/zkapi-server/src/nullifier_store.rs)
- root/signature configuration: [`rust/crates/zkapi-server/src/config.rs`](../../rust/crates/zkapi-server/src/config.rs), [`rust/crates/zkapi-server/src/signer.rs`](../../rust/crates/zkapi-server/src/signer.rs)
- HTTP routing: [`rust/crates/zkapi-server/src/routes.rs`](../../rust/crates/zkapi-server/src/routes.rs)

The previous fixed-seed/fixed-height server bootstrap has been removed in favor of config-driven seeds, epoch, tree height, and initial root.

## Challenge Planner

`rust/crates/zkapi-server/src/watcher.rs` now reconstructs challenge payloads from archived finalized request transcripts.

That means the server persists enough material to answer a stale escape withdrawal with:

- the original request public inputs
- the archived proof artifact
- the note-specific restore siblings supplied by the indexer/chain watcher
