# Client privacy and recovery boundaries

`zkapi-clientd` runs on the user's computer. An OpenAI-compatible UI sends its
prompt to the loopback API; the daemon acquires a short-lived OpenRouter key
through the zkAPI proof protocol and sends inference directly to the provider.
The wallet/prover helper receives neither prompts nor responses nor model IDs.
The provider sees the inference content. Client applications may store their
own transcripts; the daemon cannot hide content already given to that UI.

Direct HTTPS is the default. It exposes the source IP to destination services
and uses local DNS, so network timing/IP can correlate activity even though
credential issuance is unlinkable. Optional Wisp carries destination TLS with
certificate verification and hides the source IP from destination services;
the relay still sees connection metadata. A configured relay failure never
falls back to direct HTTPS. Environment proxy variables are ignored.

The client keeps the same deployed manifest/contract pins, station/key binding
and verification rules. If the verifier is unavailable, only an eligible
trusted station can use the web-compatible outage exception; invalid evidence
is not an outage. A fixed warning and `verifier-unavailable` response metadata
report that exception. One verification result remains with its acquired key;
there is no background re-verification queue.

The default fixed key-reuse window is 60 seconds, configurable from 0 to 300.
The provider can link all requests using the same key, including different
chats or local clients; its original aggregate spending cap is shared. Use 0
to require fresh access on every call. Expiry, errors and cancellation discard
cached access. The cache is memory-only and does not survive a restart.

Inference is loopback-only and key-free by default. Other local processes can
use it. Optional local API-key authentication affects only the UI-to-daemon hop.
Browser Origins, unexpected Host values and nonloopback peers are rejected.
Wallet routes always require both the local API credential and a separate
owner-only management credential. Incoming cookies, identity headers and
account/storage metadata are stripped before inference.

A local Ethereum signing key and private recovery journals are stored in
owner-only files. Public funding addresses, amounts, withdrawal destinations
and chain transactions are visible to RPC providers and chain observers.
Deposit/withdrawal QR codes are generated locally and contain only public
payment address, chain and amount. No external QR service receives them.
A withdrawal fee QR funds the local signing address, never the private payout.

Waiting for ETH and refreshing quotes do not authorize transactions. Enter
approves the displayed operation and fee ceiling; a fresh quote is checked
before signing. Network/deployment/address/note/payout/nonce bindings are
preserved. Increased fees or renewed shortage require another Enter. A lost
reply is recovered from durable state, not a blind second approval. Signed
pending operations resume their exact bytes; reverts never automatically
sign a replacement. Existing wallets are preserved across updates.

The daemon creates no request-history log files. Foreground output includes
HTTP route/method/status/timing and locally numbered key-session starts/ends,
with signed-settlement cost and remaining ETH balance. Terminal redirection
or service managers can retain that activity and financial metadata. Prompts,
responses, secrets, raw session identifiers, proofs and provider credentials
are not printed. Session events are a bounded in-memory feed, not a ledger.

Only coarse reviewed model spending buckets reach the helper. The client
continues reading the existing public `/chat/model-tickets` pricing-policy
endpoint, which is also used by the web client; that does not add ticket
wallet, issuance or redemption support. Exact balances and prompt sizes do
not choose the bucket.

The protocol is experimental and uses the existing single-party Groth16
setup. See the repository's [note-binding review](../../docs/note-binding-review.md)
and [native ETH architecture](../../docs/architecture.md). This repository move
does not deploy servers, change circuits or establish new live acceptance.
