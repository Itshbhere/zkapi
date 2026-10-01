# Native ETH operator deployment

This image runs the native ETH API, indexer and escape challenger with the exact
`protocol/setup/v2` artifacts. Supply the matching vault/signing keys and pinned
native oracle configuration.

Build on a Linux VM with Docker Engine and Compose, from the complete source
tree, including the in-repository `protocol/` source:

```sh
sudo install -d -m 0700 /etc/zkapi
sudo install -d -m 0700 -o 10001 -g 10001 /var/lib/zkapi
sudo docker compose -f docker/aws/compose.yml --profile challenge build server gateway signer
```

Create these root-owned, mode `0600` files without printing their contents:

| File | Contents |
| --- | --- |
| `/etc/zkapi/deployment.env` | Copy `deployment.env.example`, then supply vault address, chain, deployment block, gwei request cap, `ZKAPI_NATIVE_BILLING_RPC_URL`, `ZKAPI_NATIVE_PRICE_FEED_ADDRESS`, feed decimals and freshness policy. |
| `/etc/zkapi/indexer.env` | `RPC_URL`, the chain's read RPC endpoint. |
| `/etc/zkapi/server.env` | `ZKAPI_STATE_SEED`, `ZKAPI_CLEAR_SEED`, and `ZKAPI_OPENROUTER_MANAGEMENT_KEY` for direct leases; alternatively set `OA_ORG_URL` in deployment metadata and `ZKAPI_OA_ORG_SHARED_SECRET` here. For private Sepolia access, also set `ZKAPI_TESTNET_PASSWORD` here only. |
| `/etc/zkapi/challenge.env` | `ZKAPI_CHALLENGE_RPC_URL=http://signer:8547` and `ZKAPI_CHALLENGE_SENDER`, the exclusive funded sender managed by the private sidecar. |
| `/etc/zkapi/signer.env` | `ZKAPI_CHALLENGE_PRIVATE_KEY`, `ZKAPI_CHALLENGE_CHAIN_ID=11155111`, `ZKAPI_CHALLENGE_RPC_URL` (the public Sepolia upstream RPC), and `ZKAPI_CHALLENGE_VAULT` (the configured vault). Only the signer container receives this file. |

Also create `/etc/zkapi/config.json` with mode `0644`: this is the public client
manifest containing the contract, signing-key, circuit and proving-key
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

For password-gated Sepolia, forward `X-ZKAPI-Testnet-Password`, `Origin`, and
both `Access-Control-Request-*` headers through CloudFront. Preserve no-store
and CORS response headers and allow OPTIONS. The gate runs in the API process;
neither an alternate proxy route nor a direct daemon connection bypasses it.
See [access configuration and rotation](../../docs/testnet-auth.md). Never log
the password header or place the password in the public client manifest.

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
configured vault and the intended signing coordinates. Compare the server root and
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

## Validation

Run the local SDK, Rust, Solidity, launcher and signer tests before rollout.
Use a separately authorized native ETH lifecycle check for the intended network;
read-only health/quote checks alone do not establish paid inference, withdrawal
or live escape-challenge success. Preserve server data, checkpoints and signer
funding throughout any deployment change.
