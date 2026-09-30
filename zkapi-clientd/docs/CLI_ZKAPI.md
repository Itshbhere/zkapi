# Client configuration and use

The public commands are `zkapi-clientd config` and `zkapi-clientd serve`.
The client only supports zkAPI private ETH balances. Ticket issuance, import,
redemption and ticket-based inference are not included.

## Configuration

A new `config` creates the private configuration directory and defaults to
Ethereum Mainnet, direct HTTPS and a loopback listener at `127.0.0.1:8787`.
It asks for the deposit amount in USD, defaulting to $20 on Enter, and shows the
ETH amount, receiving address and locally generated QR. It waits automatically
for ETH and updates the receiving balance. Once funded, Enter approves the
shown principal and maximum network fee. It checks the quote again before
signing, deposits and waits for finalized activation. No model is selected
or priced during funding.

If the last fee or balance check leaves the deposit unsigned, configuration
refreshes the same fixed deposit and continues watching instead of exiting.
Interrupted approval responses are checked against saved progress before any
retry. A higher fee ceiling or renewed funding shortage requires Enter again.
The recommended fee buffer remains optional.

After signing, progress distinguishes waiting to be mined, waiting for network
fees to fall below the signed cap, Ethereum finality, and local balance
activation. In an interactive terminal, payment details and progress update in
place, with a spinner and elapsed time while waiting. Enlarge a short terminal
to show the payment QR; the receiving address and amounts remain available as
text. Prompts pause the display while you answer. Redirected output and
`TERM=dumb` use plain status messages without animation. The same signed
transaction is retried; the client never silently raises its fee cap or sends a
second deposit. Finality typically takes about 15 minutes **after mining**, so
time spent waiting for inclusion is additional. The web wallet currently
activates at a mined receipt; the CLI keeps its stricter finalized-receipt check.

```sh
zkapi-clientd config --status           # show saved settings
zkapi-clientd config --edit             # change settings
zkapi-clientd config --usd 50           # amount for a new deposit
zkapi-clientd config --menu             # withdrawal and wallet actions
zkapi-clientd config --require-api-key  # require a local inference key
zkapi-clientd config --api-key          # show that key when requested
```

Use the arrow keys (or `j`/`k`) and Enter to select wallet actions and networks.
Esc or `q` cancels a selection. Text prompts remain available when output is
redirected. Ctrl+C stops safely and preserves saved progress; run
`zkapi-clientd config` again to continue.

`serve` does not prompt or authorize funding. Missing prerequisites point back
to `config`. Configuration stops temporary services it starts; an existing
compatible service it reused remains running. Stop a running service before
editing its configuration.

## Sepolia

```sh
zkapi-clientd config --network sepolia
zkapi-clientd serve
```

Sepolia requires the deployment's shared access password; configuration asks
with input hidden. Fund with Sepolia ETH, never mainnet ETH. Mainnet and
Sepolia have separate signing keys and wallet state. Switching networks keeps
the other network's recovery data.

Sepolia is pinned to the September 30 fresh deployment used by
<https://oa-wallet-eth-sepolia.vercel.app/>:
`https://sepolia.100.21.48.23.sslip.io/config.json`, chain `11155111`, vault
`0x49fA19f9bdECe7A48Ebc7749fD69aD40F577590F`. The embedded manifest also pins
its new state/clearance signing keys and the unchanged proof setup. The OA
issuer, verifier and provider routing remain unchanged. The mainnet client pin
remains `https://54.67.93.98.sslip.io/config.json`; this change updates only
Sepolia. Installing the client does not deploy a server or contract.

Preserve old Sepolia profiles and the matching earlier client for recovery.
A fresh deployment needs a separate `--config-dir`; saved manifests and funding
records are never silently rebound. Do not delete an existing profile to get
past a deployment mismatch. The September 30 change is source-only until a
new client bundle is explicitly published; installing an older release retains
its older embedded deployment.

## Withdrawals

Choose `withdraw` in `config --menu`. A new withdrawal asks for the destination
once and shows its full private payout. If gas is insufficient, it shows the
same address/QR and live balance UI as a deposit, but requests only public ETH
for fees. The QR pays the local signing address, not the withdrawal destination.
Partial incoming payments update the remaining top-up. Normal gas fluctuations
within the displayed buffer do not repeat the QR.

When funded, Enter approves the displayed operation and maximum fee. A fresh
quote checks the same chain, deployment, address, private note, payout, nonce
and recovery binding before signing. Higher fees or a new balance shortage
require another Enter. The optional fee buffer does not block an otherwise
funded transaction.

Ctrl+C preserves saved progress. A signed pending withdrawal resumes its exact
transaction without another approval. A reverted transaction is never retried
automatically. Explicit recovery offers a reviewed retry or confirmation of a
matching payout submitted independently. Public ETH return remains a separate
wallet menu action with its own approval. Return quotes expire after 30 seconds;
if one expires before signing, run the same wallet menu again and review a
fresh quote. A saved signed return resumes its original transaction instead.

Returning `all` requires an ordinary Ethereum account with no deployed code.
For a delegated or contract recipient, choose an exact ETH amount and leave
room for the displayed maximum fee. The exact-amount path still simulates the
transfer and requires destination, amount and fee approval.

## Inference and activity

Use base URL `http://127.0.0.1:8787/v1`. No inference API key is required by
default; a placeholder is accepted if a client UI insists on a value. Admin
and wallet routes remain authenticated with separate local credentials.
Browser Origins are rejected. The listener must be loopback.

Open WebUI running directly on the same machine can use that URL. A Docker
container has its own loopback: use host networking where supported and enabled,
or run the client in the same network namespace. `host.docker.internal` alone
does not make a loopback-only listener reachable. Remote/container access is not
a reason to expose an unauthenticated listener on all interfaces.

Requests are serialized, including concurrent chat/title requests. Nearby
requests can share an OpenRouter key for a fixed 60 seconds by default; the
aggregate key spending cap is shared. Each request checks current model policy.
The provider can link calls sharing that key. Set `key_reuse_window_seconds`
in `config.json` to an integer from 0 to 300, or use
`config --key-reuse-window-seconds N`; 0 requests a fresh key for every call.
A new key can still wait for earlier lease settlement. Inference is never
retried automatically after a provider/transport error.

Normal output shows HTTP inference requests, key-session starts and settled
ends, actual session cost and remaining private balance in ETH. Routine helper
readiness/retry messages are hidden. Session cost appears after signed
settlement, which can take several minutes. The foreground daemon stops if its
helper exits; only an external service manager can restart it.

## Private files and existing wallets

New profiles use the OS user configuration directory plus `zkapi-clientd`:
`~/.config/zkapi-clientd` on Linux or
`~/Library/Application Support/zkapi-clientd` on macOS.
`ZKAPI_CLIENTD_CONFIG_DIR` or global `--config-dir` selects another directory.
Existing OA Chat zkAPI profiles can be reused without moving keys or recovery
journals. See [migration and updates](CLI_PACKAGING.md#existing-oa-chat-wallets).
Never initialize a new profile over orphaned recovery state or reinterpret
legacy token balances as ETH. Keep the matching historical client for recovery
of retired token wallets.
