# Reviewed pre-migration browser profiles

These public deployment profiles are copied verbatim from
`OpenAnonymity/oa-chat` commit `de53408c20b101d1c1597730a3f065980b3e9c37`,
`deployments/zkapi/fresh-20260928/{mainnet,sepolia}.json`.

They independently check the daemon's existing deployment pins during its
move to this repository. Current SDK packaging uses different CDN origins;
that does not authorize changing the client's live endpoints during a rename.
These files contain no credentials or wallet state.
