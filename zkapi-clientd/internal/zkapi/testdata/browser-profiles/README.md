# Browser deployment profile fixtures

These public files are copied verbatim from `OpenAnonymity/oa-chat`:

- `mainnet.json`: commit `de53408c20b101d1c1597730a3f065980b3e9c37`,
  `deployments/zkapi/fresh-20260928/mainnet.json`.
- `sepolia.json`: commit `856a714adab16d5529dd1f25342f007dbe254d81`,
  `deployments/zkapi/fresh-20260930/sepolia.json`.

The fixtures independently check the client's embedded deployment identity,
public signing keys, origins and proof hashes. They contain no credentials or
user wallet state. A Sepolia pin update does not authorize rebinding existing
wallets or changing the mainnet client deployment.
