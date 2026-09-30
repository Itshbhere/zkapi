# Current implementation notes

## 2026-09-30: Recoverable deposit approval and accurate progress

- An approval may return unsigned `waiting_funds` with no error when the last
  fee/balance check needs more ETH. Guided setup previously passed that state to
  signed recovery, causing the misleading "saved deposit transaction is missing"
  exit. It now refreshes the same bound deposit and waits in the same command.
  Lost approval replies are inspected through the serialized durable-state
  endpoint before deciding between unsigned retry and exact signed recovery.
- Amount, address, chain, deployment, contract, commitment and nonce remain
  bound across unsigned quote refreshes; prior approval only applies within its
  fee ceiling. Renewed shortages or higher ceilings require Enter again. The
  recommended fee reserve remains optional; reverts never retry automatically.
- Deposit progress is transient observation, separate from the durable funding
  phase. It distinguishes pending inclusion, a base fee above the signed cap,
  mined but unfinalized, finalized but awaiting activation, and active. RPC
  uncertainty does not invent successful mining or finality. Normal progress
  refreshes once a minute, without remote error text or private recovery data.
- CLI success still requires a canonical finalized receipt. The web SDK's
  successful deposit path installs its note after a mined receipt; its finality
  guards apply to failed/ambiguous retries. The indexer follows head and the
  issuer uses its current root, so web inference can begin before finality.
  The CLI's roughly 15-minute finality wait begins after mining, not signing;
  a transaction whose fee cap falls below the base fee can wait longer first.
  This change does not alter the web, indexer, issuer, contracts or helper.
- Validation: full Go race suite, vet and Linux amd64 build pass. Native
  install/upgrade/reinstall checks preserve private state and prior bundles.
  Independent review approved after correcting journal-only status to report
  unknown inclusion, with regressions for receipt failures after mining.
  Testing uses fixtures; live investigation only reads public transaction status.

## 2026-09-30: One-command binary installation

- The short README now leads with the pinned `clientd-v0.1.0` binary installer
  from `OpenAnonymity/zkapi`, followed by `zkapi-clientd config` and `serve`.
  It contains no build prerequisites, checkout steps or PATH exports. The user
  explicitly requested assuming PATH is already configured. Installer success
  output follows that assumption; internal setup still selects its matched helper.
- The command supports installation and updates, validates native archives with
  SHA-256, preserves private state and previous bundles, and leaves setup separate.
  The client uses a prerelease-specific URL instead of repository-wide `latest`.
- macOS installer CI exposed Bash 3.2 empty-array handling in the optional source
  installer. Its tests now use the system Bash, with explicit empty-argument and
  failure-propagation coverage. This does not change wallet or server behavior.
- Published [clientd-v0.1.0](https://github.com/OpenAnonymity/zkapi/releases/tag/clientd-v0.1.0)
  as a prerelease from `3ab395aab8832ec467f3afb49cc427018d3f343d`. All four
  native builds, install/reinstall tests, package checks and release assembly
  passed in [CI](https://github.com/OpenAnonymity/zkapi/actions/runs/36698217866).
  All 12 public asset downloads match the reviewed checksums and exact source.
  The public piped installer passed fresh-install/update tests on Darwin arm64
  in an isolated prefix, preserving private state and previous bundles with
  no build or PATH instructions. See [release evidence](releases/clientd-0.1.0.json).
  No live inference, wallet transaction or actual user installation was changed.

## 2026-09-30: zkapi-clientd migration from OA Chat

- `zkapi-clientd/` is the client-side Go daemon formerly at
  `OpenAnonymity/oa-chat/daemon`, imported from commit
  `de53408c20b101d1c1597730a3f065980b3e9c37`. The frontend executable is
  `zkapi-clientd`; its packaged Rust wallet/prover executable is `zkapi-walletd`.
  Public commands are `config` and `serve`. Ticket wallet/import/redemption,
  ticket inference and mode-selection flags/prompts are removed. The fixed
  `backend: zkapi` field remains for existing-profile and local-status compatibility.
- [The short README](../zkapi-clientd/README.md) is the end-user mainnet quick
  start. [Client details](../zkapi-clientd/docs/CLI_ZKAPI.md),
  [packaging](../zkapi-clientd/docs/CLI_PACKAGING.md), and
  [privacy](../zkapi-clientd/docs/PRIVACY.md) cover advanced settings and recovery.
  End users install prebuilt bundles with a one-command installer. Source builds
  remain optional developer tooling. Client release tags use `clientd-v`,
  separately from SDK/operator tags.
- Default mainnet/direct transport, $20-default deposit prompt, terminal payment
  QR and automatic balance waiting, fee-bound Enter confirmation, guided
  withdrawal, loopback key-free API, serial request queue, 60-second key reuse,
  trusted-station verifier outage policy and settled session cost logs carry over.
  The public model-policy endpoint still has `/chat/model-tickets` in its name;
  it supplies model tiers, not tickets or user identity.
- Deployment JSON, live service URLs, proof assets, bridge header/environment ABI
  and private recovery formats are unchanged. The current operator Rust workspace
  deliberately removed its old local wallet. Client builds therefore prepare
  pinned historical companion/protocol commits plus exact reviewed patches in a
  separate directory. Do not apply those patches to root Rust or regenerate keys.
  The sole patch change for this move is a CLI name in a safe password error.
- New profiles use OS config `zkapi-clientd`. If absent, the client recognizes a
  private existing OA Chat zkAPI profile and uses it in place without creating
  credentials or moving wallet state. Explicit `--config-dir`/new environment
  selection wins; the old environment override remains a fallback. Ticket,
  malformed or orphaned profiles fail closed. Installation does not start,
  fund or migrate a wallet; old installed clients remain available for recovery.
- The original repository removes its daemon and client workflows and links here.
  Historical validation records remain pinned to its pre-move Git commit, not
  rewritten as new-client evidence. No live inference or wallet transaction is
  part of this migration, and no server or contract is redeployed.
- Validation: full Go race suite and vet pass, Linux amd64 cross-build passes,
  and the real Darwin arm64 Go/Rust bundle builds with exact pinned-source
  verification. Native installation and reinstallation pass in an isolated
  prefix, retaining private-state sentinels and prior bundles. Installer (30),
  packaging (11), source orchestration (4) and source preparation (1) regressions
  pass; new workflow YAML and shell/Python syntax checks pass. Fresh adversarial
  review approved after CI and installation-prerequisite corrections. No live
  inference/funding/withdrawal or actual user installation was performed.
