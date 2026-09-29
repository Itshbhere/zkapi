# Single-VM deployment

This ERC20 review-deployment example runs the updated API, indexer and escape challenger from one image
with the exact `protocol/setup/v2` artifacts included. It requires a fresh
`zkapi-v2-note-bound-v1` verifier/vault and the seeds whose public coordinates
were used in that vault. See [deployment](../../docs/deployment.md) and
[challenge operation](../../docs/challenge-service.md).

The existing native-ETH deployment uses its own native-billing launcher and public
manifest. Do not replace that launcher or vault with this ERC20 example; native
pricing, gwei billing, signing keys, and persistent state must match the deployment.

Build on a Linux VM with Docker Engine and Compose, from the complete source
tree (including the in-repository `protocol/` source and uncommitted integration
changes, if those are the intended release):

```sh
sudo install -d -m 0700 /etc/zkapi
sudo install -d -m 0700 -o 10001 -g 10001 /var/lib/zkapi
sudo docker compose -f docker/aws/compose.yml --profile challenge build server gateway signer
```

Create these root-owned, mode `0600` files without printing their contents:

| File | Contents |
| --- | --- |
| `/etc/zkapi/deployment.env` | Copy `deployment.env.example`, then supply fresh vault address, chain and deployment block. |
| `/etc/zkapi/indexer.env` | `RPC_URL`, the chain's read RPC endpoint. |
| `/etc/zkapi/server.env` | `ZKAPI_STATE_SEED`, `ZKAPI_CLEAR_SEED`, and `ZKAPI_OPENROUTER_INFERENCE_KEY` for metered proxy requests. For direct leases, add `ZKAPI_OPENROUTER_MANAGEMENT_KEY`; alternatively set `OA_ORG_URL` in deployment metadata and `ZKAPI_OA_ORG_SHARED_SECRET` here. |
| `/etc/zkapi/challenge.env` | `ZKAPI_CHALLENGE_RPC_URL=http://signer:8547` and `ZKAPI_CHALLENGE_SENDER`, the exclusive funded sender managed by the private sidecar. |
| `/etc/zkapi/signer.env` | `ZKAPI_CHALLENGE_PRIVATE_KEY`, `ZKAPI_CHALLENGE_CHAIN_ID=11155111`, `ZKAPI_CHALLENGE_RPC_URL` (the public Sepolia upstream RPC), and `ZKAPI_CHALLENGE_VAULT` (the fresh vault). Only the signer container receives this file. |

Also create `/etc/zkapi/config.json` with mode `0644`: this is the public client
manifest containing the fresh contract, signing-key, circuit and proving-key
hash pins. The gateway mounts it read-only and serves exactly `/config.json`,
`/proofs/request.pk`, `/proofs/withdrawal.pk`, and `/proofs/manifest.json` as static
artifacts. Proving keys are copied into the gateway image from the same checkout
as the daemon image. Other files and directories are not served.

Compose reads these as environment files; quote values containing `$`, `#` or
other Compose env-file syntax appropriately. Keep credentials out of compose
command arguments and images. `docker compose config` expands secrets, so use
`config --quiet` for validation.

Start the complete deployment after configuring the signer:

```sh
sudo docker compose -f docker/aws/compose.yml --profile challenge config --quiet
sudo docker compose -f docker/aws/compose.yml --profile challenge up -d --wait
sudo docker compose -f docker/aws/compose.yml --profile challenge ps
```

The challenge profile is explicit so image building and local startup checks
can run before a signer is available. Omitting it starts only the API, indexer
and gateway; such a deployment does not have escape-challenge protection. The
challenger requires RPC `eth_sendTransaction`; an ordinary public Sepolia RPC
cannot act as its signer. The included restricted signer sidecar requires an explicit supported chain
(11155111 for this Sepolia example) and accepts only challenge calls to its
configured vault. Its chain must match the RPC, public manifest and daemon. Never publish its RPC port.
Keep the funded sender exclusive to this daemon and deployment.

The default HTTP origin binds `127.0.0.1:8080`. For an EC2 origin behind
CloudFront, set `ZKAPI_HTTP_BIND=0.0.0.0` and `ZKAPI_HTTP_PORT=80` for Compose,
restrict inbound traffic with the EC2 security group, and provide HTTPS at the
public edge. Set CloudFront caching disabled and forward all query strings,
required request headers, and methods for API/indexer paths. No daemon or signer
port should be exposed by the VM security group. Neither SQLite data nor the
operator dashboard is served by the gateway.

Optional Compose settings are `ZKAPI_IMAGE_TAG`, `ZKAPI_CONFIG_DIR`, and
`ZKAPI_DATA_DIR`. Persist these settings in the operator's root-owned Compose
environment file and use the same settings on each deployment command.

## Acceptance checks

```sh
curl --fail http://127.0.0.1:8080/health
curl --fail http://127.0.0.1:8080/v1/attestation
curl --fail http://127.0.0.1:8080/v1/tree/root
sudo docker compose -f docker/aws/compose.yml logs --tail=50 challenger
```

Verify that health and attestation report protocol 2, the selected chain, the
fresh vault and the intended signing coordinates. Compare the server root and
indexer root against the vault's current root. Verify that requests to
`/v1/dashboard/recent` and `/v1/dashboard/events` return 404 through the public
origin and HTTPS edge. The API health endpoint measures process health;
successful chain synchronization and challenge transaction submission require
the independent checks and live test flow described in the deployment docs.

All services restart after a VM reboot when Docker is enabled. Database,
indexer cursor and challenge checkpoint live in `/var/lib/zkapi`; preserve and
back up that directory across image updates. The challenger shares the API's
SQLite database and must use the same host filesystem for SQLite locking. A
recreated image must use the same setup artifacts as the deployed verifier.

## Live Sepolia acceptance

`scripts/accept-sepolia.py` drives the native client against the public HTTPS
endpoint using an encrypted Foundry keystore. Use a funded acceptance account
distinct from both the treasury and dedicated challenge sender, and an
affordable configured model. It mints only the deployment's freely mintable
demo token, approves the exact deposit total, executes one completion limited
to eight output tokens, and checks cooperative withdrawal and treasury payout.

```sh
scripts/accept-sepolia.py \
  --deployment https://PUBLIC_DISTRIBUTION/config.json \
  --zkapi /absolute/path/to/zkapi \
  --keystore /private/acceptance-account.json \
  --password-file /private/acceptance-password \
  --run-dir /private/new-acceptance-run \
  --model CONFIGURED_PROVIDER_MODEL \
  --challenge
```

The optional `--challenge` flow snapshots note A before its one request, deposits
a second note to change the tree root, and submits an escape from A's stale
snapshot. It waits for the deployed challenge daemon to restore A with the
historical request proof, verifies the confirmed on-chain challenge event,
then cooperatively closes both notes. This tests the live challenge path without
waiting for the 24-hour finalization window. The ordinary flow sends four
acceptance-account transactions; `--challenge` sends seven, plus the daemon's
challenge transaction. Run it against an otherwise idle fresh test vault so
balance and root checks are unambiguous.

Acceptance transactions use legacy fees at 125% of the current RPC gas price with an
explicit gas limit 20% above the live estimate. `--gas-price-wei` fixes that
price; `--max-gas-price-wei` caps it (default 3 gwei). Each send checks the
account balance against the maximum upfront fee and records its estimate.
Set `--wait-for-gas-seconds 900` to allow a top-up to arrive during a run: the
helper polls only the balance every ten seconds, then refreshes the gas estimate
and price before signing. The default is to stop immediately on insufficient ETH.
Each transaction's exact nonce and call are saved before asynchronous submission,
and its hash is saved immediately afterward. Receipt polling lasts up to 900
seconds (`--receipt-timeout-seconds` overrides this). An unresolved transaction
retains its records and blocks further sends; reconcile or replace that exact
nonce before restarting a run. The helper never automatically resubmits it.
At roughly 1 gwei, budget around 0.06 ETH for the acceptance account and
0.02 ETH for the separate challenge sender for the complete challenge flow;
actual requirements vary with gas prices. The challenge daemon independently
uses its live estimate plus 20%; the signer's default 12-million gas ceiling
accommodates estimates up to 10 million. A 6.9-million-gas estimate therefore
needs about 8.3 million gas of upfront capacity, not merely the expected final
charge. The script reports bounded, sanitized `cast` error details on failure.

The helper verifies the manifest/on-chain signing keys, cap and token, public
proof files, private dashboard blocking, and indexer/server/contract root
agreement before funding. `ETH_RPC_URL` can override the manifest's read RPC
for chain calls. Keep the full source checkout and matching setup available;
the native client's proof loader uses `protocol/setup/v2` relative to it. All
wallets, generated proofs, logs, receipts and `summary.json` are preserved in
the new mode-0700 run directory. A failure stops local client processes but
does not delete wallet recovery state or initiate additional transactions.
