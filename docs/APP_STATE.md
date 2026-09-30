# Current implementation notes

## 2026-09-30: Successful client deposits activate at the mined receipt

- At the user's explicit request, successful native ETH deposits now activate
  after validating the canonical mined receipt and its saved deposit/event
  binding, matching the web SDK. They no longer wait for a finalized block.
  Progress goes from pending inclusion (or signed fee-cap waiting) to balance
  activation and reports "Deposit confirmed" on completion.
- A failed receipt retains the finality gate before explicit retry. Missing or
  ambiguous receipts never authorize a replacement; recovery keeps the exact
  saved transaction. The transient `failed_finalizing` stage distinguishes this
  failure wait. The old `finalizing` label remains neutral for configuration
  attached to an older daemon. The interactive in-place display, spinner,
  cancellation cleanup and plain-output fallback are preserved. Withdrawal and
  public-return finality checks are unchanged.
- This accepts the same pre-finality reorganization exposure as the web wallet.
  The web confirmation installs its active note and clears `pendingDeposit`;
  the CLI also treats active notes as committed. Neither supplies complete
  post-activation rollback. The indexer advances a block-number cursor and tree
  snapshot without canonical-hash rollback; root divergence is logged rather
  than automatically reconciled. Local/indexed state can diverge from the vault,
  and a local reset cannot undo inference credit already issued.
- Canonical receipt validation does not claim full reorganization recovery.
  Event validation consumes the same receipt whose block was checked. If
  inclusion becomes unavailable or invalid before activation, saved
  `confirming` progress returns to `deposit_pending` while preserving the
  signed transaction. A local activation outage retains recoverable activation
  progress; it never reinitializes an already active balance.
  No operator service, indexer, contract, circuit, helper or live deployment
  changes are required. [Client configuration](../zkapi-clientd/docs/CLI_ZKAPI.md) and
  [recovery boundaries](../zkapi-clientd/docs/PRIVACY.md) describe the behavior.
- Validation: full Go race suite, vet and Linux amd64 build pass. Regression
  tests cover mined activation, canonical/event guards, provisional failures,
  changing receipts before activation, and exact transaction recovery.
  Independent review approved after correcting pre-activation progress.
  This change performs no live wallet transaction.

## 2026-09-30: Fresh Sepolia client deployment pins

- Update only the client embedded Sepolia manifest, manifest URL, canonical
  browser-profile fixture and existing QR test pins to
  `zkapi-native-eth-sepolia-note-bound-v1-fresh-20260930`. Chain 11155111 uses
  vault `0x49fA19f9bdECe7A48Ebc7749fD69aD40F577590F` and server
  `https://sepolia.100.21.48.23.sslip.io`. The four proof hashes and the
  pinned historical Rust helper remain unchanged; no runtime trust override,
  verifier bypass, mainnet update or automatic wallet migration is added.
- Preserve old deployment profiles and their matching client. An isolated new
  configuration is required for the fresh Sepolia wallet; see
  [client configuration](../zkapi-clientd/docs/CLI_ZKAPI.md#sepolia).
- An isolated native Darwin arm64 test build uses these exact four source
  files, matching installed helper/patch provenance and all proof hashes.
  `CGO_ENABLED=0 go test ./...`, vet and build pass; installer (30), source
  preparation (1) and packaging (11) tests pass. The local Xcode license prevented
  full local race testing; [CI](https://github.com/OpenAnonymity/zkapi/actions/runs/36767392475)
  passed race tests and vet on both Ubuntu and macOS for the exact tested source.
  App, contracts, Rust and Docker checks also passed. Independent review
  approved the narrow pin change.
- Live Sepolia CLI acceptance passed: isolated funding and deposit finalized,
  the model catalog loaded, and one short inference returned HTTP 200 with
  station verification marked verified and nonempty response content. Exact
  response text was not asserted. Settlement, private withdrawal and unused
  public ETH return completed with finalized receipts. A graceful restart
  resumed the same saved return transaction without another approval; a later
  restart confirmed the same funding account, zero private balance, no active
  note or pending inference, and completed wallet operations. Only a prudent
  public fee reserve remains. Test services were stopped and the private
  profile retained. Exact amounts and wallet/transaction identifiers stay in
  protected operational evidence. No release has been published or installed,
  and existing user wallets/binaries remain unchanged.
- Return acceptance covered the ordinary exact-amount path for a delegated
  recipient. An expired quote failed closed without signing; a fresh quote was
  reviewed before submission. The CLI's quote and recipient guards were not
  changed. [Client details](../zkapi-clientd/docs/CLI_ZKAPI.md#withdrawals)
  describe this existing behavior and preservation of signed recovery state.


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
  mined but unfinalized, finalized but awaiting activation, and active in that
  revision. Successful native deposits now skip that finality stage as described
  above. RPC uncertainty does not invent successful mining or finality. Normal
  progress refreshes once a minute, without remote error text or private recovery data.
- At that revision, CLI success required a canonical finalized receipt (now
  superseded by mined-receipt activation above). The web SDK's
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
  withdrawal, loopback key-free API, serial request queue, configurable key reuse,
  trusted-station verifier outage policy and settled session cost logs carry over.
  New profiles and profiles without a saved reuse window use a fixed 60-second
  window, allowing compatible requests across chats and local clients to share
  a key and its spending cap. Set the window to 0 for a fresh key per inference
  request; explicitly saved windows retain their existing behavior.
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
