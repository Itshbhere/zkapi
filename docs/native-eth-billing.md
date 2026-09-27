# Native ETH browser billing

Native ETH deployments hold ETH and use integer **gwei** in the existing proof
ledger: one unit is 1,000,000,000 wei. The browser displays an approximate USD reference value;
the USD value changes with ETH's price. This is not a stable-dollar deposit or a
swap into USDC. Fractional gwei are never credited to a private note.

The native vault is a fresh deployment with `billingToken() == address(0)` and
`nativeAssetWeiPerUnit() == 1000000000`. Its existing deposit selector accepts
integer gwei and requires exactly `amount * 1 gwei` in `msg.value`. Wrong values
revert atomically. Native deposits are capped at JavaScript's maximum exact
integer, 9,007,199,254,740,991 units. Withdrawal and expiry payouts convert ledger
units back to wei and retain the existing proof, nullifier, destination, timing
and reentrancy protections. Failed recipient transfers revert the complete close.

Nonzero-token deployments retain the ERC-20 behavior and reject ETH on deposit.
Existing vaults are immutable and do not gain native support from a frontend
update. Publish a fresh deployment ID, contract address and native asset pins;
preserve the legacy wallet and withdrawal route for old notes. This asset change
retains the pinned circuit and proof keys: the circuit operates on integer
balances and does not assign a currency denomination. It does not include the
separate note-binding circuit migration.

## Pinned configuration

The public deployment manifest and browser trust configuration must agree on:

```json
{
  "billing_asset": "native_eth",
  "billing_unit": "gwei",
  "billing_token_address": null,
  "native_asset_wei_per_unit": "1000000000",
  "native_price_feed_address": "0x694AA1769357215DE4FAC081bf1f309aDC325306",
  "native_price_feed_decimals": 8,
  "native_price_max_age_seconds": 4500
}
```

Pin the RPC URL, chain, fresh vault and ordinary protocol/signing/proof-asset
fields too. The example feed is Sepolia; it does not select a production feed.
The server checks chain ID, feed decimals and the vault's native-unit getter
before answering or accepting a quote. There is no symbol-only fallback.

Chainlink's reference directory lists the ordinary ETH/USD proxies as
`0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419` on Mainnet and
`0x694AA1769357215DE4FAC081bf1f309aDC325306` on Sepolia, both eight decimals
with a 3,600-second heartbeat, checked on 2026-09-27. The server and SDK read
oracle rounds at the RPC `finalized` block tag; the USD reference can therefore
lag the chain head, and `updated_at` identifies the actual feed observation.
The native deployment pins a 4,500-second maximum age: the heartbeat plus a
900-second finality allowance. Freshness remains configurable and fails closed
if finality or feed updates lag beyond the pinned allowance. The RPC must support
`finalized`; there is no fallback to head/latest pricing. Quotes with a future
timestamp, incomplete round, nonpositive price or expired timestamp fail.
Review feed provenance and freshness policy before each deployment.

Sources: [official documentation feed-directory configuration](https://github.com/smartcontractkit/documentation/blob/main/src/features/data/chains.ts#L294),
[Chainlink API](https://docs.chain.link/data-feeds/api-reference),
[Mainnet directory](https://reference-data-directory.vercel.app/feeds-mainnet.json),
[Sepolia directory](https://reference-data-directory.vercel.app/feeds-ethereum-testnet-sepolia.json).

## A quote belongs to one private lease

`GET /v2/billing/quote` is a read-only, `Cache-Control: no-store` endpoint:

```json
{
  "asset": "native_eth",
  "units_per_eth": 1000000000,
  "chain_id": 11155111,
  "feed_address": "0x694aa1769357215de4fac081bf1f309adc325306",
  "round_id": "123",
  "answer": "250000000000",
  "decimals": 8,
  "updated_at": 1700000000,
  "expires_at": 1700004500
}
```

This is a schema example, not a usable current quote. Round and price are
canonical decimal strings. `expires_at = updated_at + configured max age`.
The browser independently verifies the round through the pinned RPC and adds
this entire object as `billing_quote` to the existing prompt-free lease
authorization before generating a proof. The canonical payload hash and request
authorization tag bind the quote to that request and its proof. It carries no
prompt, account identity, funding address or wallet secret.

Before reserving a new nullifier, the server verifies the quoted round with
`getRoundData` and requires it to equal `latestRoundData` on the pinned feed,
both read at `finalized`, as well as checking freshness. A user cannot choose a
favorable historical round. Head-only updates do not invalidate a prepared
finalized quote. If a newer round finalizes while a proof is being prepared,
new issuance rejects it with `native_quote_superseded`; the exact old request
must be recovered before constructing a new proof. The exact accepted request,
including its quote, is persisted before upstream key provisioning. The rate is
never refreshed on a matching retry or during settlement. In particular, a
restart or an oracle outage cannot reprice an existing lease.

For ledger units `U`, oracle integer answer `P`, and decimals `D`, the upstream
budget in micro-USD is:

`floor(U * P * 1,000,000 / (1,000,000,000 * 10^D))`.

The OA org still receives and signs micro-USD limits and measured usage; it does
not need native-token pricing support. For measured usage `C` in micro-USD the
native charge is:

`ceil(C * 1,000,000,000 * 10^D / (P * 1,000,000))`.

Zero usage stays zero. Integer conversion is checked for overflow and browser
safe bounds. The signed state transition charges gwei and remains capped by the
proof's solvency bound. Issuance and the settlement response payload echo the
exact quote so the browser can compare it with its persisted journal.

A `native_quote_expired` response is emitted only before any nullifier
reservation. Freshness is rechecked after all oracle awaits and after proof
verification immediately before reservation, with equality treated as expired.
Issuance attempts share one lock, so an exact POST that returns this rejection
has also waited for earlier in-flight attempts. A local clock or an unknown GET
alone does not authorize deleting a journal; startup remains read-only.
`POST /v2/openrouter/leases/{id}/expire` accepts the exact saved request and waits
on the same issuance lock. It returns `expired_unaccepted` when the server clock
says expired, or `superseded_unaccepted` when a newer finalized round exists,
with the exact request ID, nullifier and payload hash. Both acknowledgments
require that neither its nullifier nor request ID has a reservation. Superseded
recovery relies on Ethereum finality: an ordinary head reorganization cannot
revive the old quote. A finalized-chain safety violation is outside this model,
as it is for the private note's confirmed chain state. This endpoint is
read-only: it never generates a key, signs, reserves or cancels anything. Every
other result preserves the journal. It allows interrupted startup/withdrawal recovery without
starting a chat solely to test whether an old proof is still issuable. Matching
reserved/provisioning requests retain their old quote and remain recoverable
after expiry. Other errors do not authorize discarding a
pending journal. Native `/v2/requests` proxy billing is explicitly disabled;
only prompt-private leases currently have native settlement support. Legacy
ERC-20 leases reject native quote fields and preserve their old conversion.

## Deployment and current scope

The deploy script accepts `NATIVE_ETH=true`, `MINT_AMOUNT=0`, zero/absent
`BILLING_TOKEN`, and an explicit `REQUEST_CHARGE_CAP` in gwei. It checks an
optional `CHAIN_ID` against the connected chain. These flags require a new vault;
they must not overwrite a live legacy deployment's manifest.

Start the native server with its fresh vault/chain/request cap and ordinary
signer, indexer, proof and OA-source settings, plus:

```sh
zkapi ... serverd ... \
  --native-billing-rpc-url "$RPC_URL" \
  --native-price-feed-address "$ETH_USD_FEED" \
  --native-price-feed-decimals 8 \
  --native-price-max-age-seconds 4500
```

The server requires prompt-private lease configuration and rejects proxy policy
for native mode. Configure the published manifest separately; these flags do
not rewrite static frontend manifests. Current native integration targets the
browser SDK. The existing local CLI/clientd funding flow still expects ERC-20
and does not prepare native quotes; it must not be advertised as native-ready.

Validation includes legacy replay-mutation regressions, native quote identity,
RPC chain/round/freshness verification, rounding and overflow checks, persisted
quote recovery, expiry-check serialization behind in-flight issuance, oracle
reads crossing expiry, finalized-vs-head round advancement and superseded recovery,
and real browser proof/authorization binding with a signed
gwei settlement. Contract tests cover native deposit/close/escape/expiry payouts,
wrong-value rejection, failed-recipient atomicity and recipient reentrancy. Native ETH deployment and
live end-to-end acceptance are separate from these local tests.
