# Browser deployment profile fixtures

These public profiles use `OpenAnonymity/oa-chat` commit
`856a714adab16d5529dd1f25342f007dbe254d81`,
`deployments/zkapi/fresh-20260930/{mainnet,sepolia}.json`, with only their
manifest, protocol and indexer hostname fields changed to
`zkapi-mainnet.openanonymity.ai` and `zkapi-sepolia.openanonymity.ai`.
The SDK default profiles contain the same reviewed deployment configuration.

The fixtures independently check the client's embedded deployment identity,
public signing keys, origins and proof hashes. They contain no credentials or
user wallet state. Mainnet now selects the already deployed September 30
vault; existing September 28 wallets are preserved for their matching client.
Only the exact September 30 Sepolia manifest permits an origin-only update.
