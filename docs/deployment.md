# Native ETH deployment

The source supports native ETH vaults and the note-bound Groth16 circuit
`zkapi-v2-note-bound-v1`. The current selected setup remains a single-party
experimental setup; publishing code does not establish an independent audit or
setup ceremony. See [setup provenance](../protocol/setup/v2/README.md).

For an existing deployment, use its exact circuit artifacts, contract and signing
key pins, oracle feed, denomination, and persistent operator state. Do not
regenerate keys or copy wallet journals between deployments.

## Build services

```sh
cargo build --release --workspace
```

`zkapi serverd` requires `--native-billing-rpc-url` and
`--native-price-feed-address` (or corresponding `ZKAPI_NATIVE_*` variables), plus
an OA org/service credential or direct OpenRouter management credential.
Supply chain ID, vault, gwei request cap, matching signing seeds, database path,
indexer URL and the exact proof directory. Run `zkapi serverd --help` for flags.
Native prices come from the pinned finalized oracle round; a generic RPC/chain
or USD token configuration is not interchangeable with these pins.

For a private Sepolia test, configure `ZKAPI_TESTNET_PASSWORD` only in the
Sepolia server's secret environment. The built-in gate requires that password
for every service API call while leaving health and protocol metadata public.
See [Sepolia shared password](testnet-auth.md) for client behavior, proxy header
forwarding, validation and rotation. Never put this value in the public SDK
manifest or configure it on Mainnet.

## Fresh contracts

`demo/contracts/script/Deploy.s.sol` deploys the real Groth16 adapter and a
native-only vault. It reads `PRIVATE_KEY`, `TREASURY`, `OUTPUT_PATH`, state and
clearance public-key coordinates, optional `CHAIN_ID`, `REQUEST_CHARGE_CAP`
and `CHALLENGE_PERIOD_SECONDS`. The deployment output is constructor metadata;
prepare the public SDK manifest with circuit/key hashes and oracle/deployment
pins separately. The vault no longer accepts a token constructor argument.

Use a fresh directory for any intentional development setup. Existing immutable
contracts are not modified by this source cleanup. Keep challenger coverage and
persistent server/indexer state for every funded deployment during a rollout.

## Runtime

The [Docker guide](../docker/aws/README.md) configures API, indexer, gateway,
challenger and a restricted private signer. The challenger needs a funded
exclusive account and must fit RPC/indexer/retry delays inside the challenge
window; see [challenge operation](challenge-service.md).

The host application packages the SDK assets for the selected network. Review
its trusted config against finalized on-chain deployment state, the public
manifest and the selected proof hashes before enabling funding.

Historical deployment records in `docs/deployments/` describe their recorded
revisions and observations. They are not instructions to restart retired token
clients or assertions that a later source revision is already deployed.
