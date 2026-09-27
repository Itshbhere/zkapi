# Note-bound native deployment requirement

The prior circuit allowed a server-signed balance commitment from one active note
to be transplanted into another note. The repaired `zkapi-v2-note-bound-v1` circuit
binds the same private note leaf used in Merkle membership into the signed
commitment: `C = balance * G + blinding * H + leaf * J`. The independently derived
third generator has no known scalar relation to G or H. A fresh random blinding
continues to hide the leaf from the server. This adds the usual discrete-log
binding assumption for J alongside the existing Poseidon, Schnorr and Groth16
assumptions; it is not a formal cryptographic audit.

The JSON v2 schema/public-input order remains compatible, but legacy signed
states, WASM and proving/verifying keys are not. New key files carry a circuit
header that rejects accidental use of old files. Independently pinned key hashes
and fresh immutable verifier/vault addresses remain mandatory. Publish and pin
`proof_setup.circuit_id` / `trusted_deployment.circuit_id` as
`zkapi-v2-note-bound-v1` only for this exact setup. Legacy notes need their old
client and deployment for recovery; never rewrite their signed state.

The existing repaired setup is a single-party development setup with recorded
source, not an independently reviewed multiparty ceremony. Mainnet launch remains
subject to explicit acceptance of those setup and audit limitations. Native ETH
does not remove them.

The vault also preserves the archived request's historical active root when
challenging an escape after unrelated tree updates; only the restoration path
uses the current root. The [v2 challenge daemon](challenge-service.md) reconstructs
that exact accepted request proof and retries durable submissions. A daemon with
the wrong circuit or the old historical-root guard cannot protect the vault.

This integration ports protocol repair `49164f68cbfbc65eda66a35bcaab2400952699e2`
onto the native-ETH contract and ports the compatible watcher/daemon/CLI circuit
guard from the immutable September22 source archive (SHA256
`9082e16c47e7918a07f7d114ca5424ba9b8af50e5edba8c6c160843afef5ed00`). It preserves the
reviewed native finalized-oracle pricing and expiry/supersession recovery. It does
not claim unrelated direct-OpenRouter retirement/proxy concurrency changes from
that archive were ported; the native web deployment uses OA signed final receipts
and explicitly rejects proxy billing.

Required regressions include equal/unequal-note signature transplant rejection,
real browser native authorization and signed gwei settlement under the revised
setup, real historical-root challenges, native deposit/payout atomicity, and v2
challenge submission/restart/same-nonce recovery. Keep local test results separate
from deployment and live end-to-end acceptance evidence.
