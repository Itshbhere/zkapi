# Native ETH / Groth16 source cleanup

This change keeps the native ETH browser wallet and the v2 Groth16/BN254,
Poseidon and Baby-JubJub proof/signature path. It removes the retired STARK/XMSS
and token-payment implementations from the source tree.

## Removed

- Cairo circuits, Scarb/STARK tooling, XMSS/WOTS and Stark-curve commitment code.
- V1 proof/wire types, epoch-root configuration, old protocol server/CLI and
  duplicate indexer; the generic v2 Rust wallet remains.
- The token-only local wallet daemon, funding/withdrawal CLI commands, unused
  alternate-auth crate and old integration/demo/acceptance scripts.
- SDK token mint/approve/allowance/balance and token transaction recovery paths.
- Docs-site test-token minting faucet, API, credentials template and signing dependency.
- Server inference proxy/providers and nonnative billing fallbacks. Runtime
  leases require native oracle configuration and a proof-bound billing quote.
- Vault token transfers, token constructor options and token mocks. Required
  OpenZeppelin contracts and forge-std source helpers are retained with licenses;
  unused vendor tests, token libraries and nested tool repositories are removed.

## Compatibility

The circuit ID, request/withdrawal public inputs, commitment/signature rules,
proof-coordinate encoding, proving/verifying keys and packaged WASM are unchanged.
The setup remains the same single-party experimental setup, not a new ceremony.

Current native ETH Mainnet/Sepolia manifests retain their separate deployment
pins. The SDK rejects token manifests and legacy/unidentified wallet state before
using it as native state. It does not rewrite private notes or convert old
balances. Historical token clients remain available at earlier Git revisions,
including `1aed24e74aee24700f0d011b95f58a2c4ec3eb9b`.

The new source vault has no token constructor parameter. Its existing native
runtime methods, `billingToken() == address(0)` and
`nativeAssetWeiPerUnit() == 1 gwei` remain compatible with native clients. This
source edit does not redeploy or modify existing immutable contracts. A new
constructor/runtime bytecode must be reviewed and pinned for any future deploy.

The current operator CLI retains `setup`, `signing-keys`, `serverd` and `indexer`.
Server startup requires native RPC/feed configuration and a lease issuer;
`zkapi-clientd`, token funding commands and `POST /v2/requests` are removed.
Settlement recovery GET routes remain. Existing SQLite rows/checkpoints are not
reset; compatibility storage columns may remain so native recovery can read
existing databases without destructive migration.

The reviewer changelog describes its pinned pre-cleanup snapshot. Historical
review and deployment records are retained as evidence, with obsolete operation
instructions labeled historical. Current usage is in the repository README,
SDK README and native deployment guide.

## Validation

Validated locally:

- Root Rust release workspace: 66 tests passed.
- Protocol Rust release workspace: 49 tests passed.
- Both Rust workspaces: formatting and Clippy with warnings denied passed.
- Solidity: 29 tests passed; deployment script builds.
- SDK: 295 tests passed; Sepolia and Mainnet builds passed.
- Shipped WASM request and escape-withdrawal proofs verify natively.
- SDK artifact hashes verified; setup keys and packaged WASM are unchanged.
- Operator launcher: 4 tests passed; restricted signer: 11 tests passed.
- Docs site: production build and type checks passed after removing the token faucet.

The original sequential A/B cross-note regression and real historical-root
challenge proofs remain in those suites. Native-only startup and legacy wallet
rejection are explicitly tested. A Docker image build could not run locally
because the Docker daemon is unavailable; CI retains that build. The source
cleanup does not establish new live chain/provider acceptance or change the
setup's trust assumptions.
