# Protocol Map

This chapter ties the protocol sections to the code.

## Registration

Protocol:

- user samples `secret_s`
- computes `C = Poseidon(domain("zkapi.reg"), secret_s, 0)`
- deposits on-chain

Implementation:

- Rust client registration helper: [`rust/crates/zkapi-client/src/wallet.rs`](../../rust/crates/zkapi-client/src/wallet.rs)
- registration commitment helper: [`rust/crates/zkapi-core/src/leaf.rs`](../../rust/crates/zkapi-core/src/leaf.rs)
- note leaf hash: [`rust/crates/zkapi-core/src/leaf.rs`](../../rust/crates/zkapi-core/src/leaf.rs)
- Solidity deposit path: [`contracts/src/ZkApiVault.sol`](../../contracts/src/ZkApiVault.sol)
- Solidity leaf/hash logic: [`contracts/src/libraries/NoteLeafLib.sol`](../../contracts/src/libraries/NoteLeafLib.sol), [`contracts/src/libraries/MerkleUpdateLib.sol`](../../contracts/src/libraries/MerkleUpdateLib.sol)

## Request Proof

Protocol:

- prove membership of the active note
- prove either genesis or a valid signed state
- derive request nullifier
- reveal only anonymized commitment and solvency bound

Implementation:

- Cairo request program: [`cairo/src/request/program.cairo`](../../cairo/src/request/program.cairo)
- Rust witness builder and verifier: [`rust/crates/zkapi-proof/src/request.rs`](../../rust/crates/zkapi-proof/src/request.rs)
- shared public inputs: [`rust/crates/zkapi-types/src/inputs.rs`](../../rust/crates/zkapi-types/src/inputs.rs)

## Server Processing

Protocol:

- verify proof
- reserve nullifier
- execute provider call
- apply charge
- derive next anchor and blind delta
- sign next state with PQ XMSS

Implementation:

- request processor: [`rust/crates/zkapi-server/src/processor.rs`](../../rust/crates/zkapi-server/src/processor.rs)
- provider boundary: [`rust/crates/zkapi-server/src/provider.rs`](../../rust/crates/zkapi-server/src/provider.rs)
- transcript storage: [`rust/crates/zkapi-server/src/nullifier_store.rs`](../../rust/crates/zkapi-server/src/nullifier_store.rs)
- signer: [`rust/crates/zkapi-server/src/signer.rs`](../../rust/crates/zkapi-server/src/signer.rs)

## Withdrawal

Protocol:

- prove ownership of the current note state
- mutual close requires a server clearance signature
- escape hatch freezes the note immediately
- stale escape withdrawals can be challenged with a more recent request proof

Implementation:

- Cairo withdrawal program: [`cairo/src/withdrawal/program.cairo`](../../cairo/src/withdrawal/program.cairo)
- Solidity settlement and challenge logic: [`contracts/src/ZkApiVault.sol`](../../contracts/src/ZkApiVault.sol)
- client withdrawal construction: [`rust/crates/zkapi-client/src/wallet.rs`](../../rust/crates/zkapi-client/src/wallet.rs)
- challenge payload planner: [`rust/crates/zkapi-server/src/watcher.rs`](../../rust/crates/zkapi-server/src/watcher.rs)

## Merkle and Hash Layer

Protocol:

- note leaves use `Poseidon(domain("zkapi.leaf"), ...)`
- internal nodes use `Poseidon(domain("zkapi.node"), left, right)`

Implementation:

- Cairo Merkle: [`cairo/src/merkle.cairo`](../../cairo/src/merkle.cairo)
- Rust Merkle: [`rust/crates/zkapi-core/src/merkle.rs`](../../rust/crates/zkapi-core/src/merkle.rs)
- Solidity Poseidon and tree update: [`contracts/src/libraries/StarkPoseidon.sol`](../../contracts/src/libraries/StarkPoseidon.sol), [`contracts/src/libraries/MerkleUpdateLib.sol`](../../contracts/src/libraries/MerkleUpdateLib.sol)
