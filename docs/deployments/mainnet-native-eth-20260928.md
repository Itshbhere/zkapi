# Native ETH Mainnet rollout — September 28, 2026

## Contracts finalized; backend and public application published

The user explicitly authorized public Mainnet deposits on September 28 after
reviewing the experimental deployment limitations. The native vault and proof
adapter are deployed and verified at finalized chain state. The live backend,
dedicated challenger, public manifest and oracle checks passed. The SDK now
removes Mainnet's `deployment_status: "migration_required"` guard. The host
application pins that immutable SDK release at its separate native Mainnet
origin below.

| Field | Verified value |
|---|---|
| Chain | Ethereum Mainnet, 1 |
| Deployment ID | `zkapi-native-eth-mainnet-note-bound-v1-20260928` |
| Finalized native vault | `0x9e5570ae0F1FCB087c2dD0eac521aC067a6b6F42` |
| Finalized proof adapter | `0x7C530D1eeab639FB78DAefCEdb4E0AA046727898` |
| RPC | `https://ethereum-rpc.publicnode.com` |
| Manifest | `https://d3hmaz52qw22t.cloudfront.net/config.json` |
| ETH/USD feed | `0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419` |
| Oracle policy | 8 decimals, finalized reads, maximum age 4,500 seconds |
| Billing | Native ETH, integer gwei, 1,000,000,000 wei per unit; no token |
| Circuit | `zkapi-v2-note-bound-v1` |
| Protocol revision | `8b2d4e3da921f956e1eb6b93afbf722a877c060c` |
| Backend binary source | `2e9647cecec78e1258b34f8be1bc85ea2378cd66` |

Finalized block `26074697` includes both deployments:

| Contract | Deployment block | Transaction | Actual fee (ETH) |
|---|---:|---|---:|
| Proof adapter | 26074584 | [`0xa0c30975…`](https://etherscan.io/tx/0xa0c3097513d8eaddb4e1c3ef2cadb23b69405289523c44b0015ff5a5cbf1c1c6) | 0.000235550335345579 |
| Native vault | 26074671 | [`0x80a3bcd5…`](https://etherscan.io/tx/0x80a3bcd5303a841ebe8ce42661ce0ebb9eeaf19425acb8867d8dd164dd8cb1e7) | 0.000834420181935360 |

Total deployment fee was **0.001069970517280939 ETH**. Finalized runtime code,
constructor getters, independent signing keys, native denomination and oracle
configuration match the reviewed release. At this check the vault had zero
balance and zero notes; the separately funded challenger held 0.005 ETH.
These observations establish deployment identity, not completed user flows.

The contract signing keys are independent Mainnet public keys pinned in
`sdk/assets/config/mainnet.json`. The setup, SDK WASM and proving artifacts are
unchanged from the reviewed native Sepolia version. Only Mainnet configuration,
its generated artifact digest, the corresponding deployment checks and these
docs change. A later SDK configuration commit does not change backend binary
provenance; keep those revisions separate in release records.

Activation checks established agreement among finalized vault constructor getters,
public manifest, native API quote, operational dedicated challenger and packaged
SDK pins. Only Mainnet's configuration guard is removed; the generic legacy
migration guard and its negative tests remain. Regenerate and verify
`sdk/assets/manifest.json` after every configuration change before building the
host's immutable SDK dependency.

## Backend verification

At `2026-09-28T08:49:18.851Z`, the public CloudFront origin
`https://d3hmaz52qw22t.cloudfront.net` passed the actual SDK's manifest and trust
validation and independent finalized ETH/USD quote verification. Its manifest
SHA-256 is `355c674f641f11edff0eaa29a985122c22b42126f99d2d998a7b7ad49ade29ff`.
Distribution `E301WJL60HXXBI` reached Deployed with the reviewed configuration.
Public `/signer`, `/.env` and `/v1/dashboard/recent` each returned 404.
Authenticated origin TLS passed six negative authorization checks; native
Sepolia's application containers and configuration were unchanged.

The dedicated Mainnet challenger advanced its durable checkpoint past block
`26074834` with no pending submissions. Its 0.005 ETH reserve exceeded the
then-current 10,000,000-gas maximum fee estimate of 0.00183578607 ETH. These are
timestamped observations, not a guarantee of future affordability; reserves
must be replenished as gas prices and concurrent escape demand change.

The reviewed preparation passed 305 SDK tests. After removing only the Mainnet
configuration guard, all 24 focused artifact, packaging and migration tests
passed, along with artifact verification and the Mainnet SDK build. A fresh
adversarial review approved the source. The backend checks above were read-only;
they did not submit a user deposit, inference request or challenge transaction.

## Published application

The canonical native Mainnet application is
**https://oa-wallet-eth-mainnet.vercel.app**, Vercel deployment
`dpl_EfvfCXhrcPYR5DVh8aQ8UCb5HXso`, build `K2DVDIBU`, app revision
`c81405722608a32f2883c115a8ed8e522e6ba12b`, SDK revision
`cf56d67e0c1dd4bc3f3c32392478ce242c746446`. All 486 published artifact hashes
matched that build at `2026-09-28T08:58:52Z`; canonical config, health,
attestation/root and independently verified finalized native quote checks
passed. Operator-only routes remained inaccessible.

A fresh unfunded browser profile opened zkAPI without account sign-in, showed
both wallet choices, and rendered the Mainnet address, ETH total and approximate
USD total with fee details closed by default. Four USD/ETH switches completed
in 9.3–15.5 ms and preserved the exact principal. An explicitly entered
0.002345678 ETH amount, its input denomination and funding address survived both
a page reload and a full browser restart. Next remained disabled; public RPC
reported zero balance and zero confirmed/pending nonces for the test address.
No uncaught JavaScript page errors appeared. A bounded reopen/reload follow-up
identified a handled proxy-preference warning: the app could not disable the
proxy while requests were in progress. Funding quotes and input recovery still
worked. Two console errors from an earlier restart did not reproduce; their
cause was not established. This does not establish a console-clean or
reliable-proxy run.

No Mainnet deposit, withdrawal, inference request or extension confirmation was
performed during that browser check. These results establish onboarding,
quotation and local persistence, not a paid Mainnet end-to-end happy path.

## Separation and limitations

The native Sepolia configuration, live vault, signing keys and existing browser
wallets are unchanged. Legacy Mainnet USDC vault
`0xef88012d1A7F9d44e5f5afB8bC5e611Dc3283709`, its original server
`https://d27v1dvkaxfc09.cloudfront.net`, and its recovery frontend
`https://oa-wallet-mainnet.vercel.app` remain separate. Retain their wallet state
and recovery client; do not reinterpret a token note as ETH or copy its journal
into this deployment. Moving a frontend to a new origin does not migrate local
browser wallet data.

This is an explicitly authorized experimental public deployment. The proof setup
is single-party, the circuit/Rust/Solidity integration is unaudited, and isolated
live escape-challenge acceptance remains incomplete. Local regression tests and
Sepolia's successful deposit/inference/mutual-close acceptance do not establish
that missing live test. No Mainnet browser transaction or paid inference test
is claimed by this configuration work.

The vault accepts permissionless deposits without a total-deposit or TVL cap.
The 50,000-gwei request-charge cap is not a cap on user deposits or operator
liability. ETH balances fluctuate in USD value. Challenger reserves require
monitoring and replenishment; a frontend rollback must not disable challenge
coverage or rewind durable server state while notes or leases remain active.
Pausing also prevents mutual close and escape initiation, so it is not a
harmless deposit-only switch for a funded vault.

Private notes and signing material remain browser-local. Protocol and RPC
requests omit account cookies; proof requests contain no prompts. Inference
continues directly from the browser to OpenRouter with OA-issued short-lived
keys and the pinned verifier. No verifier bypass is enabled.
