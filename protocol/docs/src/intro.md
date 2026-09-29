# Introduction

This book explains how the repository implements the zkAPI protocol described in [`PROTOCOL.md`](../../PROTOCOL.md) and pinned by [`SPEC.md`](../../SPEC.md).

It is written as an implementation companion:

- `PROTOCOL.md` explains the protocol model and threat model.
- `SPEC.md` defines the required system behavior and interfaces.
- This book maps those requirements to concrete code in Cairo, Solidity, and Rust.

## Repository Structure

```text
cairo/      Cairo proof programs and PQ signature logic
contracts/  Solidity vault and proof adapters
rust/       Client SDK, server, proof artifacts, indexer, crypto helpers
```

## Security Model

The codebase follows the intended split:

- Post-quantum primitives are used for proofs, Merkle hashing, nullifiers, and state signatures.
- The one accepted non-PQ exception remains the balance commitment on the Stark curve.
- The on-chain Merkle and leaf hashing now match the Cairo and Rust implementations through a Cairo-compatible Poseidon implementation in Solidity.

## Reading Order

If you want the fastest path through the implementation:

1. Read [Protocol Map](protocol-map.md).
2. Read [Cairo Programs](cairo.md) to understand what is actually proven.
3. Read [Solidity Contracts](contracts.md) to understand settlement and escape-hatch safety.
4. Read [Rust Services and SDK](rust.md) for the operational state machines.
5. Read [End-to-End Flows](flows.md) for the full lifecycle.
