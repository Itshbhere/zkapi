# Testing and Verification

From the repository root:

```bash
cargo test --release --manifest-path protocol/rust/Cargo.toml --workspace --locked
cargo test --release --workspace --locked
```

Protocol tests cover canonical fields and serialization, Merkle trees, native
wallet persistence, browser wallet state, compact proof encoding, commitment
binding, and request/withdrawal circuit constraints. Root tests cover the
active runtime HTTP paths, nullifier persistence, and challenge processing.

From `protocol/contracts/`, run `forge test` for deposit, native settlement,
withdrawal, challenge, and real Groth16 verifier regressions.

The SDK asset check is `node scripts/sync-sdk-assets.mjs --check` from the repository
root. Keep the committed WASM and key hashes synchronized with the setup
manifest. Key generation is a separate operation and is not required for this
cleanup or ordinary verification. The existing
[development setup limitations](../../setup/v2/README.md) still apply.
