# Cairo Programs

The Cairo layer is the proving core of the system.

## Modules

- top-level library: [`cairo/src/lib.cairo`](../../cairo/src/lib.cairo)
- domain tags: [`cairo/src/domains.cairo`](../../cairo/src/domains.cairo)
- protocol constants: [`cairo/src/constants.cairo`](../../cairo/src/constants.cairo)
- Merkle verification: [`cairo/src/merkle.cairo`](../../cairo/src/merkle.cairo)
- balance commitment: [`cairo/src/pedersen_balance.cairo`](../../cairo/src/pedersen_balance.cairo)
- PQ signatures: [`cairo/src/xmss.cairo`](../../cairo/src/xmss.cairo)
- request program: [`cairo/src/request/program.cairo`](../../cairo/src/request/program.cairo)
- withdrawal program: [`cairo/src/withdrawal/program.cairo`](../../cairo/src/withdrawal/program.cairo)

## Request Program

`run_request_program` proves:

- the registered note exists in the active tree
- the prover knows `secret_s`
- the nullifier is derived from the current anchor
- the current state is either genesis or signed by the server
- the anonymized commitment corresponds to the hidden balance and blinding
- the hidden balance is at least the public solvency bound

The public output order is intentionally aligned with the Rust and Solidity `RequestPublicInputs` structs.

## Withdrawal Program

`run_withdrawal_program` proves:

- the note exists in the active tree
- the withdrawal nullifier is derived from the current anchor
- the hidden final balance is not above the original deposit
- the current state is either genesis or signed by the server
- optional mutual-close clearance was signed by the server

The withdrawal circuit reveals `note_id` because the contract needs a concrete leaf to remove from the active-note tree.

## XMSS / WOTS+

The state-signature system is fully hash-based:

- WOTS+ chain function lives in [`cairo/src/xmss/wots.cairo`](../../cairo/src/xmss/wots.cairo)
- XMSS authentication path logic lives in [`cairo/src/xmss/tree.cairo`](../../cairo/src/xmss/tree.cairo)
- combined signature verification lives in [`cairo/src/xmss/verify.cairo`](../../cairo/src/xmss/verify.cairo)

This is the post-quantum authentication layer used for state updates and mutual-close clearances.

## Pedersen Exception

The one accepted non-PQ exception is the balance commitment:

`E(balance, blinding) = balance * G_balance + blinding * H_blind`

Implementation:

- Cairo commitment code: [`cairo/src/pedersen_balance.cairo`](../../cairo/src/pedersen_balance.cairo)
- Rust reference implementation: [`rust/crates/zkapi-crypto/src/pedersen.rs`](../../rust/crates/zkapi-crypto/src/pedersen.rs)

The Cairo coordinates are now pinned to the exact Rust-derived generators, so the commitment layer is cross-language consistent.
