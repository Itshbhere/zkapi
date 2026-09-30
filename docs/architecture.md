# Native ETH architecture

```text
Host app -> browser SDK -> zkAPI server (proof-backed lease authorization)
                   |                  -> OA org/station or direct key management
                   |-> inference provider (prompts/responses)
                   |-> Ethereum native ETH vault
                   |-> indexer (current Merkle root and paths)

Challenger -> durable server transcripts + indexer -> restricted signer -> vault
```

The browser keeps note secrets, blindings, signing evidence and recovery journals.
It creates Groth16 proofs locally with the packaged WASM and matching keys.
The server verifies each exact authorization, reserves its nullifier, issues a
bounded key and persists settlement before returning a new signed balance.

The authorization includes a pinned finalized ETH/USD quote. Notes and signed
balances are integer gwei; provider costs are micro-USD converted using that
saved quote. Exact retries preserve the original proof and quote. See
[native billing](native-eth-billing.md) for rounding and recovery rules.

The indexer mirrors deposits, closes and escape transitions. The challenger
preserves historical request proofs while obtaining current restoration paths,
and retries through durable deployment-bound checkpoints. See
[challenge operation](challenge-service.md).

Only the v2 Groth16/Baby-JubJub proof path and native ETH payment path are active.
The operator CLI supplies server/indexer/setup/signing-key commands; the host
application integrates the browser SDK directly. The separate [zkapi-clientd](../zkapi-clientd/README.md) frontend provides a local
OpenAI-compatible API backed by private native ETH balances. It packages a
pinned historical Rust wallet/prover with reviewed patches, separately from
this operator workspace. It has no ticket mode or token wallet.
