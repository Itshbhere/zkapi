# Reviewed browser deployment profiles

These public files are copied verbatim from `OpenAnonymity/oa-chat`:

- `mainnet.json`: commit `de53408c20b101d1c1597730a3f065980b3e9c37`,
  `deployments/zkapi/fresh-20260928/mainnet.json`. This remains the historical
  mainnet client deployment pin carried through the repository migration.
- `sepolia.json`: commit `856a714adab16d5529dd1f25342f007dbe254d81`,
  `deployments/zkapi/fresh-20260930/sepolia.json`. This is the fresh deployment
  profile used by the canonical Sepolia browser app after the September 30
  rollout, including its new vault, server origin and signing-key pins.

The fixtures independently check the client's embedded deployment identity,
public signing keys, origins and proof hashes. They contain no credentials or
user wallet state. A Sepolia pin update does not authorize rebinding existing
wallets or changing the mainnet client deployment.
