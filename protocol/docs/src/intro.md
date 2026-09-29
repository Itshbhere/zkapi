# Introduction

This book maps the active zkAPI v2 implementation to its source. The
[protocol model](../../PROTOCOL.md) describes sequential anonymous spending;
the [implementation specification](../../SPEC.md) describes exact interfaces.

The current implementation uses BN254 Groth16 proofs, BN254 Poseidon hashing,
and Baby-JubJub Schnorr signatures and note-bound balance commitments. The
Solidity vault settles native ETH in whole gwei. These are classical
elliptic-curve primitives. Review the [development setup assumptions and
compatibility](../../setup/v2/README.md) before deploying.

```text
contracts/  Native ETH vault and Groth16 verification
setup/v2/   Matching circuit-specific development keys
rust/       Proof circuits, native/browser wallets, types, and Merkle helpers
../crates/  Runtime server, challenge service, indexer, and operator CLI
../sdk/     Browser SDK and proof assets
```
