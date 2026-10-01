# Browser deployment profile fixtures

These public profiles match the reviewed deployment configurations packaged in
`sdk/assets/config/{mainnet,sepolia}.json`. Their public endpoints are
`zkapi-mainnet.openanonymity.ai` and `zkapi-sepolia.openanonymity.ai`.

The fixtures independently check the client's embedded deployment identity,
public signing keys, origins and proof hashes. They contain no credentials or
user wallet state. Mainnet selects the deployed September 30 vault; existing
September 28 wallets are preserved for their matching client. Only the exact
September 30 Sepolia manifest permits an origin-only update; both its prior
bytes and its canonical replacement are pinned by SHA-256.
