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

```sh
zkapi-clientd config --status           # show saved settings
zkapi-clientd config --edit             # change settings
zkapi-clientd config --usd 50           # amount for a new deposit
zkapi-clientd config --menu             # withdrawal and wallet actions
zkapi-clientd config --require-api-key  # require a local inference key
zkapi-clientd config --api-key          # show that key when requested
```

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

The live manifests, contracts, issuer, verifier and inference services are
unchanged from the OA Chat daemon. Mainnet uses the deployment behind
<https://staging.openanonymity.ai/>; Sepolia uses the one behind
<https://oa-wallet-eth-sepolia.vercel.app/>. Their manifest origins remain
`https://54.67.93.98.sslip.io/config.json` and
`https://52.52.207.206.sslip.io/config.json`, respectively. Deployment pins are
validated locally. No server or contract is deployed by installing the client.

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
wallet menu action with its own approval.

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
