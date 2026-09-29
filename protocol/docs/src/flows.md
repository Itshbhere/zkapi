# End-to-End Flows

## Request

1. The wallet proves membership and a note-bound state, sufficient balance,
   rerandomization, and authorization for the exact request context.
2. The server verifies deployment, signing keys, time, root, bound, and proof.
3. A durable nullifier reservation prevents competing spends.
4. Provider execution or a short-lived OpenRouter lease determines usage.
5. The server applies the bounded charge, derives fresh blinding and anchor,
   signs the next state with Schnorr, and persists the transcript.
6. The wallet verifies the returned transition and installs its next state.
   Prepared-request journals support retries and recovery.

## Mutual close

The client asks for clearance on the current withdrawal nullifier. The server
reserves it and returns a clearance signature. The client proves the final
balance and clearance, and the vault removes the note leaf and settles native
ETH to the user and treasury.

## Escape and challenge

The client can prove a withdrawal without clearance. Initiation removes the
leaf and starts the challenge period. A stale state has a nullifier already
used by an accepted request. The challenge service submits that archived request
proof and a current zero-leaf path; the vault restores the leaf. Historical
request roots remain valid evidence after unrelated note-tree changes.

After an uncontested deadline, anyone can finalize the pending withdrawal to
its previously bound destination. If an active note expires without close-out,
its deposit can be claimed for the treasury.
