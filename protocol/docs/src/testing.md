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
[setup trust assumptions and review status](../../setup/v2/README.md) still apply.

For the complete current native-v2 lifecycle, run `npm run test:e2e:v2` from the
repository root. It uses real services, proofs and contracts on a local chain,
with a mocked provider and oracle. See the
[acceptance process](../../../docs/v2-acceptance.md) for the exact boundary.
