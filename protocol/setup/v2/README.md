# Note-bound Groth16 setup

Circuit ID: `zkapi-v2-note-bound-v1`.

These are single-party Groth16 keys generated with OS randomness by
`cargo run --release -p zkapi-proof --example setup -- NEW_OUTPUT_DIRECTORY`.
The `v2` directory name denotes the wire schema. Proving/verifying files carry
the `zkapi-v2-note-bound-v1` circuit header.
The matching generated Solidity verifier is committed alongside them.

No multi-party setup ceremony has been conducted. Matching hashes establish
artifact identity, not the absence of retained setup secrets. Independently
review the circuit, third Pedersen generator, setup provenance and trust
assumptions before deployment.
The vault's immutable verifier, client proving keys, and pinned hashes must
match this setup. Use the matching circuit for all signed states and keep wallet
data bound to its configured vault and signing keys.
