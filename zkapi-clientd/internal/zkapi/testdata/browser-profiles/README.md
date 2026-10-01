# Browser deployment profile fixtures

These public profiles match the reviewed deployment configurations packaged in
`sdk/assets/config/{mainnet,sepolia}.json`. Their public endpoints are
`zkapi-mainnet.openanonymity.ai` and `zkapi-sepolia.openanonymity.ai`.

The fixtures independently check the client's embedded deployment identity,
public signing keys, origins and proof hashes. They contain no credentials or
user wallet state. The network manifests pin the current vaults and deployment
configuration; SHA-256 checks enforce exact approved manifest bytes.
