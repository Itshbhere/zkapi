# End-to-End Flows

## Request Lifecycle

```mermaid
flowchart LR
    A["Client note state"] --> B["Build request proof artifact"]
    B --> C["Server verifies proof artifact and public outputs"]
    C --> D["Reserve nullifier"]
    D --> E["Execute provider"]
    E --> F["Compute charge, blind delta, next anchor"]
    F --> G["XMSS-sign next state"]
    G --> H["Persist transcript"]
    H --> I["Client verifies response and installs next state"]
```

## Mutual Close

1. Client derives withdrawal nullifier from the current anchor.
2. Client requests server clearance.
3. Server signs the clearance message with its clearance XMSS tree.
4. Client verifies the clearance signature.
5. Client builds a withdrawal proof artifact with `has_clearance = true`.
6. Contract verifies the withdrawal statement through the adapter and settles immediately.

## Escape Hatch

1. Client builds a withdrawal proof artifact with `has_clearance = false`.
2. Contract removes the note leaf immediately and stores pending withdrawal data.
3. If the withdrawal is stale, the server reconstructs a challenge from the archived request transcript.
4. `challengeEscapeWithdrawal` restores the original leaf.
5. If no challenge arrives in time, `finalizeEscapeWithdrawal` settles the note.

## Recovery

The recovery path matters because a request consumes a nullifier before the next state is durable.

Current implementation:

- the client writes a journal before sending the request
- the journal stores the `client_request_id`, nullifier, payload hash, and `user_rerandomization`
- the server stores a transcript keyed by nullifier
- if the client crashes, it asks the server for recovery by client request id
- if the transcript is finalized, the client recomputes and verifies the exact next blinding and next state signature before installing state
