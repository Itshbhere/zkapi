# Sepolia password rollout (2026-09-29)

The fresh native ETH Sepolia service now requires the shared test password.
This updates the API process and its gateway; it does not deploy a new vault,
change signing keys or proof artifacts, or modify the Mainnet service.
See [the authentication contract](../testnet-auth.md) for client behavior and
password rotation.

| Resource | Deployed value |
| --- | --- |
| Public API | [Sepolia service](https://52.52.207.206.sslip.io/) |
| Public configuration | [config.json](https://52.52.207.206.sslip.io/config.json) |
| Web application proxy | [Sepolia health](https://oa-wallet-eth-sepolia.vercel.app/zkapi-deployment/health) |
| Chain | `11155111` |
| Native ETH vault | `0x999F40773e47f7e07f435C0CC69225c409B64329` |
| Deployment ID | `zkapi-native-eth-sepolia-note-bound-v1-fresh-20260928` |
| Server source | `ead4bfd11be72972f7dfa0f794df88e2ff7ae0c3` |
| Server image ID | `sha256:0f2b10c5487be545ef0f5a69b440997be39723b1dba49813a27dce6747fb3d39` |
| Gateway image ID | `sha256:1d734d12ef2f67ce4653c0e2886becb271b9c0d7304f4b7e8877227f973e5baf` |
| Persistent server database | `/data/zkapi-server.db` |
| Minimum lease budget | `50000` gwei units |
| Oracle | `0x694AA1769357215DE4FAC081bf1f309aDC325306`, 8 decimals, 4500-second freshness |
| Lease TTL / settlement grace / poll interval | 300 / 5 / 2 seconds |

## Deployment preservation

The Linux ARM64 server image uses the committed source above and the existing
native deployment launcher. Exactly two retired arguments were removed:
`--auth-scheme state-anchor` and `--provider metered`. All remaining arguments,
including the database path, oracle, lease policy, RPC and OA-org source, were
preserved. The gateway derives from its previous image and adds only the exact
`/v2/auth` upstream route; its proving artifacts and existing route restrictions
remain intact.

The host's legacy Docker builder required replacing `COPY --chmod=755` with
ordinary `COPY` followed by `RUN chmod 755` during image construction. The
deployment launcher was copied with mode 0755. These build adaptations preserve
file contents and executable permissions; they do not change protocol code.

Before rollout, the operator saved configuration, environment, container
metadata, indexer/challenge checkpoints and a consistent SQLite backup under
the root-owned `/root/zkapi-auth-20260929` directory. SQLite's backup API and
integrity check passed. Existing environment and mounts were retained except
for adding the shared password to the private server environment. The password
is absent from this record, public manifests, images and repository.

Only the server and gateway containers were recreated. The indexer, challenger,
restricted signer and TLS service remained running, with approximately 30 hours
of uptime. The challenger checkpoint progressed from block 11810451 to 11810460
with no pending challenge submissions. The live native database uses the same
schema; no destructive database migration or contract transaction was performed.

## Verification

The implementation revision passed all CI jobs: SDK tests and both network
builds, Rust workspace build/tests/Clippy/formatting, shipped-WASM proof
verification, protocol Rust tests/Clippy, Solidity tests and Docker build.
The authentication, web and CLI changes also received adversarial review.

Read-only acceptance ran on 2026-09-29 at approximately 22:15–22:17 UTC against
both the public API and the web application's `/zkapi-deployment/` proxy:

- Public health, attestation and indexer roots remained accessible. Health
  identifies the expected vault, `native_leases`, protocol v2 and
  `testnet_password_required: true`.
- Missing and incorrect passwords returned HTTP 401. Quote, lease,
  settlement-recovery, withdrawal-clearance and expiry routes rejected
  unauthenticated calls before processing their bodies. Duplicate credential
  headers were rejected. Authentication errors retained no-store and CORS
  response headers; browser preflight succeeded.
- The browser SDK accepted the correct password and retrieved a native ETH
  quote with the pinned Sepolia feed and freshness policy through both paths.
- The actual Go client's password loading and authentication modules rejected
  missing/wrong passwords and accepted the correct password. Bundle startup
  denied invalid credentials before launching the companion, started companion
  bridge v4 with the correct password, and retrieved and verified a native
  oracle quote. The temporary unfunded profile and both local processes were
  removed/stopped afterward.
- Both Sepolia and Mainnet public manifests remained byte-for-byte unchanged.
  Mainnet health remained healthy on chain 1 without the new password gate.

The server/indexer root remained
`0x2ab5da4324d6ce7146fbcc4e3880054401e2f73f60eb4a9e684b620ee2d5a949`.
Public proving files matched the packaged files, and the verifying-key hashes
matched the live manifest pins:

| Artifact | SHA-256 |
| --- | --- |
| `request.pk` | `c894b261a13f571d0df36be29734aabf2a8cd7162baddc5e08a50341aa076584` |
| `request.vk` | `8011244c99fa1a8524870906462d430fc86366b8ad821736c5fa726b479e6d97` |
| `withdrawal.pk` | `8e41398092fdd02b9ff86c6ccbecbd7ce2402e6f22ec162e6124d1d04fe0a668` |
| `withdrawal.vk` | `2a8ea7f07176e369a93d1d816124192a798d1466c99fd6dc47850ba82094b679` |

The operator retains credential-free acceptance records named
`acceptance-origin.json`, `acceptance-web-proxy.json`,
`post-rollout-public.json` and `cli-live-probe/live-validation.json` in the
local rollout evidence directory. These checks did not create a provider key,
issue a proof, run inference, fund a note or submit an Ethereum transaction.
They establish access control and authenticated quote retrieval, not a new
funded end-to-end inference/withdrawal acceptance run.

For an ordinary rollback, restore the previous image/configuration/environment
while retaining the current database and checkpoints. The backup predates
subsequent lease/challenger updates; restoring it requires separate state
reconciliation.
