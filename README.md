# zkAPI

zkAPI provides a browser wallet and backend for private, prepaid API access funded
with native ETH. The browser proves that a private note can cover a bounded
runtime-key lease, calls the inference provider directly, and verifies the signed
balance returned after usage settlement. The host application owns its UI.

The active protocol uses Groth16 over BN254, Poseidon, note-bound Baby-JubJub
commitments and Schnorr signatures, and a 32-level Merkle tree. The ledger uses
integer gwei. This is an experimental, unaudited protocol with a single-party
setup; see [the note-binding review](docs/note-binding-review.md).

## Local OpenAI-compatible API

Use [zkapi-clientd](zkapi-clientd/README.md) to configure an ETH wallet and serve
inference to Open WebUI or another local OpenAI-compatible client.

## Build and test

```sh
npm ci
npm test
npm run build:browser
npm run build:mainnet
npm run verify:assets
cargo build --release --workspace
cargo test --release --workspace
cargo test --release --manifest-path protocol/rust/Cargo.toml --workspace
(cd protocol/contracts && forge test)
```

The protocol and required Solidity libraries are ordinary files in this repository.
No submodule initialization, Cairo compiler or STARK prover is needed. See
[source provenance](protocol/VENDORED.md).

## Browser SDK

`@openanonymity/zkapi-browser-sdk` owns native ETH funding, wallet persistence,
proof generation, lease recovery, settlement and withdrawals. Mainnet and Sepolia
have independent deployment pins, signing keys and wallet state. Applications
configure the SDK before initializing a wallet; see [SDK integration](sdk/README.md)
and [native billing](docs/native-eth-billing.md).

The packaged WASM and proving keys support the current circuit
`zkapi-v2-note-bound-v1`. Ordinary SDK consumers do not need Rust. To rebuild
WASM deliberately, use `scripts/build-browser-client.sh` and review the changed
artifact hashes. A source cleanup does not require new proving keys.

## Operator services

- `zkapi serverd`: native ETH lease authorization, settlement and Schnorr signing.
- `zkapi-indexerd` / `zkapi indexer`: vault event indexing and Merkle paths.
- `zkapi-challenged`: durable escape monitoring and challenge submission.
- `zkapi signing-keys`: derive deployment public keys from operator seeds.
- `zkapi setup --output-dir NEW_DIRECTORY`: generate a fresh development setup.

Native server startup requires a pinned oracle RPC/feed and a lease issuer.
Use [deployment instructions](docs/deployment.md), [Docker operation](docker/aws/README.md)
and [challenge service operation](docs/challenge-service.md). The browser sends
prompts directly to the inference provider; this server does not offer a legacy
inference proxy.

The existing keys in `protocol/setup/v2` are bound to deployed verifiers. Do not
run `setup` to connect to an existing deployment. A new setup requires its own
verifier/vault and explicit client/fund migration.

## Source layout

| Directory | Purpose |
| --- | --- |
| `zkapi-clientd/` | Local ETH-funded OpenAI-compatible client and installer |
| `sdk/` | Browser native ETH wallet, public artifacts and SDK tests |
| `protocol/rust/` | Shared v2 types, BN254 helpers, circuits and Rust/WASM wallets |
| `protocol/contracts/` | Native ETH vault, Groth16 verifier and contract tests |
| `protocol/setup/v2/` | Selected circuit setup and verifier artifacts |
| `crates/` | Operator CLI, server, indexer and challenger |
| `demo/contracts/` | Native ETH verifier/vault deployment script |
| `docker/aws/` | API/indexer/challenger image configuration and restricted signer |

The retired STARK/XMSS implementation, token-payment SDK branches, local token
client and token deployment demos were removed. Historical releases remain in
Git history. Existing token wallets must use their matching historical client;
this SDK rejects them instead of reinterpreting their balances as ETH. See
[cleanup details](docs/native-only-cleanup.md).
