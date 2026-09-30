# Current implementation notes

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
  The source installer provides a working installation before binary releases
  exist. Client release tags use `clientd-v`, separately from SDK/operator tags.
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
