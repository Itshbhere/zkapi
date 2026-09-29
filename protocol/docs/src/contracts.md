# Solidity Contracts

The contract layer is the settlement and dispute engine.

## Main Vault

The central contract is [`contracts/src/ZkApiVault.sol`](../../contracts/src/ZkApiVault.sol).

Its responsibilities are:

- accept deposits and update the active-note tree
- settle cooperative withdrawals
- freeze notes immediately on escape-hatch initiation
- allow stale-withdrawal challenges
- finalize uncontested escape withdrawals after the delay
- claim expired notes for the treasury
- publish server XMSS roots by epoch

## Hash and Merkle Logic

The critical consistency fix in this repository is that Solidity no longer uses a placeholder `keccak256` tree hash.

Now:

- Poseidon implementation: [`contracts/src/libraries/StarkPoseidon.sol`](../../contracts/src/libraries/StarkPoseidon.sol)
- Merkle node hashing and path recomputation: [`contracts/src/libraries/MerkleUpdateLib.sol`](../../contracts/src/libraries/MerkleUpdateLib.sol)
- note leaf hashing: [`contracts/src/libraries/NoteLeafLib.sol`](../../contracts/src/libraries/NoteLeafLib.sol)

This makes the on-chain root compatible with Cairo/Rust witnesses.

## Proof Adapter Boundary

The vault depends on the adapter interface:

- interface: [`contracts/src/interfaces/IZkApiProofAdapter.sol`](../../contracts/src/interfaces/IZkApiProofAdapter.sol)
- fact-registry adapter: [`contracts/src/adapters/FactRegistryAdapter.sol`](../../contracts/src/adapters/FactRegistryAdapter.sol)
- mock adapter for business-logic tests: [`contracts/src/adapters/MockProofAdapter.sol`](../../contracts/src/adapters/MockProofAdapter.sol)

The adapter boundary is what lets the protocol keep Cairo proofs and still swap the exact on-chain verification backend.

## Escape Hatch Safety

The escape-hatch path now behaves as intended:

- initiation removes the note from the active tree immediately
- challenge restores the original leaf
- the stale-withdrawal nullifier remains burned after a successful challenge

That last point is important because it prevents repeated griefing with the same stale nullifier.

## Reentrancy

All state-mutating external entrypoints that transfer ERC-20 tokens are protected with `nonReentrant`, and settlement follows checks-effects-interactions ordering.
