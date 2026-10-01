# Local v2 lifecycle acceptance

This process tests the current native ETH protocol from a real deposit through
provider usage, signed settlement, a challenged stale escape, and a successful
real-proof withdrawal. The provider and price oracle are local mocks. Everything
runs on loopback with disposable Anvil accounts; no existing wallet, hosted
server, provider credential, public RPC, or testnet funds are required.

## Run

Install Node 24+, Rust, and Foundry (`forge` and `anvil` in PATH), then run from
the repository root:

```sh
npm run test:e2e:v2
```

The wrapper first checks the provider mock's accounting, installs the locked
ethers dependency, rebuilds the actual server, indexer, challenger and wallet
helper, builds the Solidity contracts, and runs the scenario.
Dependency installation/builds may contact package registries;
the acceptance scenario itself uses only loopback endpoints. It uses the
checked-in proving keys and does not generate or replace a setup.

Each run saves a private timestamped directory under `.zkapi/acceptance/` with
its JSON result, transaction receipts, service logs and server database. Override
the parent directory with `npm run test:e2e:v2 -- --output-dir /path/to/results`.
The runner shuts down its own child processes and listeners on success or
failure. Existing wallet data and deployment directories are never loaded.

On macOS the wrapper respects explicit `DEVELOPER_DIR` and `SDKROOT` values.
Otherwise, it uses installed standalone Command Line Tools and the macOS 15.4
SDK when available. It does not accept an Xcode license or change system settings.

For iteration after building, run the harness directly:

```sh
node scripts/v2-acceptance.mjs --bin-dir target/release
```

Use the wrapper for a review or release check so the binaries and contracts are
rebuilt from the current source. A successful exit and the run's JSON result are
the acceptance evidence; simply having this script does not establish a pass.

## Scenario

1. Deploy the actual Poseidon library, Groth16 verifier and native vault on a
   fresh local chain, then start the actual indexer and HTTP API server.
2. Deposit native ETH and initialize a genesis wallet state from the contract's
   note ID, amount and expiry. No earlier signed balance is manufactured.
3. Generate a real request proof, obtain a lease through the HTTP API, and send
   an inference request directly to the local provider mock using its issued key.
4. Retire the lease through HTTP. The server reads the mock's measured usage,
   settles the charge and signs the new state. The wallet verifies and applies
   that response using the production browser-wallet implementation.
5. Repeat the request/usage/settlement flow, retaining the genuine earlier signed
   state. An unrelated deposit changes the tree root.
6. Submit a real escape proof using that earlier, consumed state. The actual
   challenger discovers the event, reads the server's durable evidence, obtains
   the current path from the indexer and submits its own challenge transaction.
7. Obtain clearance for the current state through HTTP and submit its real
   mutual-withdrawal proof. Check the remaining-balance payout and treasury charge.

The vault keeps its 86,400-second challenge period. Challenges occur before that
deadline; the process does not wait a day or advance time to finalize the stale
withdrawal. Mining local confirmation blocks does not shorten the configured
window.

## Coverage boundary

| Component | Execution |
| --- | --- |
| Deposit, escape, challenge and mutual withdrawal | Actual native vault and Groth16 verifier on Anvil |
| Wallet state, proofs and settlement verification | Actual `zkapi-browser` Rust operations behind a small in-memory JSONL driver |
| Lease issuance, retirement, recovery and clearance | Actual HTTP API server and SQLite database |
| Tree paths and challenge submission | Actual indexer and challenger processes |
| Inference and key management | Local provider mock; usage accrues when an inference call is accepted |
| ETH/USD quote | Local oracle mock |
| Transaction signing and chain confirmation | Local Anvil accounts and blocks |

The harness does not establish real provider behavior, browser/CLI host
persistence or UI correctness, the AWS signer configuration, public-network
finality/reorg behavior, or live Sepolia acceptance. Those require their own
integration runs. The generated result must retain the mocked-provider label.
