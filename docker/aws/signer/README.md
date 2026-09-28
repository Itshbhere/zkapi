# Sepolia challenge signer

This internal JSON-RPC sidecar supplies `eth_sendTransaction` to
`zkapi-challenged`. It signs only canonical `challengeEscapeWithdrawal` calls
for one configured vault on Sepolia (11155111), from its dedicated key, with
zero ETH value. It decodes the complete ABI, checks the proof's deployment,
simulates the call again, and checks current gas and fee bounds before signing
with ethers. It does not expose generic signing or raw-transaction forwarding.

Required environment, supplied only to this container:

```text
ZKAPI_CHALLENGE_PRIVATE_KEY=<dedicated funded Sepolia key>
ZKAPI_CHALLENGE_RPC_URL=https://<upstream Sepolia RPC>
ZKAPI_CHALLENGE_VAULT=0x<fresh vault>
```

Bind options are `ZKAPI_CHALLENGE_LISTEN_HOST` (default `127.0.0.1`) and
`ZKAPI_CHALLENGE_PORT` (default `8547`). Compose sets the host to `0.0.0.0`
on its private backend network and publishes no signer port. Never proxy this
service to the public internet. `GET /health` checks the upstream chain and
returns only public sender/vault metadata. Use its sender as the challenge
daemon's `--sender`; point that daemon's RPC URL at `http://signer:8547`.

The default `ZKAPI_CHALLENGE_MAX_GAS` is 12,000,000 (hard ceiling 16,777,216).
`ZKAPI_CHALLENGE_MAX_GAS_PRICE_WEI` defaults to 10,000,000,000 (10 gwei; hard
ceiling 100 gwei). Calls exceeding a bound remain in the daemon's durable retry
queue. Transactions are legacy EIP-155 transactions with the current upstream
gas price; the signer does not autonomously increase fees or replace a pending
transaction. Monitor the dedicated sender's gas balance, stuck transactions,
and daemon retry/deadline logs.

The challenge daemon owns nonce reservation and its durable checkpoint. This
sidecar requires the explicit nonce and never allocates another on ambiguous
send failures. Run only one challenge daemon for the deployment and never use
its dedicated sender for other transactions. No signer state is written to
disk; the container can use a read-only filesystem. Keep the private key out of
the server/indexer/challenger environments and deployment manifests.

Run `npm ci --ignore-scripts && npm test` here. Tests use a public fixture key
and mock RPC responses, decode actual ethers signatures, and cover mismatched
deployment bindings, forbidden methods, failed simulation, fee/gas bounds,
durable nonce retries, and sanitized HTTP errors. No test broadcasts a chain
transaction.
