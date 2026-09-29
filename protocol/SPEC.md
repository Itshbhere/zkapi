# zkAPI Implementation Specification

This document is the implementation-level specification for zkAPI v1.

`PROTOCOL.md` is the protocol description.
`SPEC.md` is the build specification.
If the two documents differ, `SPEC.md` is authoritative for implementation details.

## 1. Scope

This spec defines a full v1 implementation of the zkAPI usage-credit system described in `PROTOCOL.md`.

The implementation must satisfy these constraints:

- zero-knowledge proofs are written in Cairo and use a STARK proving system;
- Ethereum contracts are written in Solidity and tested with Foundry;
- the design remains sequential: one valid state authorizes exactly one next request;
- the design is post-quantum wherever practical;
- the only explicitly accepted non-PQ exception is the homomorphic balance commitment, which may use a Pedersen-style commitment on an elliptic curve;
- the system must be modular enough to swap the proof-verification backend without rewriting the business logic contracts.

This spec covers:

- cryptographic primitives and domain separation;
- Cairo proof programs;
- Solidity contracts and state machine;
- the off-chain server;
- the client SDK and wallet state;
- Merkle tree/indexer requirements;
- wire formats and persistence;
- testing, invariants, and delivery order.

This spec does not include:

- parallel child-anchor spending;
- note renewal or rollover;
- multi-token support in one contract instance;
- frontend/UI work;
- anonymity against global network observers;
- a custom PQ homomorphic commitment scheme in v1.

## 2. High-Level Architecture

The implementation is split into six modules:

1. `cairo/`
   Contains the shared Cairo libraries plus two proof programs:
   - request proof
   - withdrawal proof

2. `contracts/`
   Contains the EVM settlement contract, Merkle update library, and proof-adapter boundary.

3. `rust/crates/zkapi-core`
   Shared types, serialization, domain constants, Merkle helpers, balance-commitment helpers, and proof/public-input builders.

4. `rust/crates/zkapi-client`
   Wallet state, note lifecycle, proof generation orchestration, persistence, and recovery logic.

5. `rust/crates/zkapi-server`
   Proof verification, nullifier storage, API execution, charge computation, PQ signing, transcript retention, and challenge handling.

6. `rust/crates/zkapi-indexer`
   Mirrors contract events into a local Merkle tree view and serves sibling paths and the latest active root.

The system is intentionally split so that:

- Cairo handles all cryptographic validity checks;
- Solidity handles note accounting and payouts;
- the server never learns the user secret or the unblinded balance;
- the contract never sees ordinary request proofs except when a stale withdrawal is challenged.

## 3. Repository Layout

The implementation should use this repository structure:

```text
/PROTOCOL.md
/SPEC.md
/README.md
/cairo
  /Scarb.toml
  /src
    /constants.cairo
    /domains.cairo
    /types.cairo
    /merkle.cairo
    /pedersen_balance.cairo
    /xmss
      /params.cairo
      /wots.cairo
      /tree.cairo
      /verify.cairo
    /request
      /public_inputs.cairo
      /witness.cairo
      /program.cairo
    /withdrawal
      /public_inputs.cairo
      /witness.cairo
      /program.cairo
/contracts
  /foundry.toml
  /src
    /ZkApiVault.sol
    /interfaces
      /IZkApiProofAdapter.sol
    /libraries
      /MerkleUpdateLib.sol
      /NoteLeafLib.sol
      /Errors.sol
      /Events.sol
      /Types.sol
    /adapters
      /MockProofAdapter.sol
      /FactRegistryAdapter.sol
  /test
/rust
  /Cargo.toml
  /crates
    /zkapi-types
    /zkapi-core
    /zkapi-crypto
    /zkapi-proof
    /zkapi-client
    /zkapi-server
    /zkapi-indexer
    /zkapi-cli
```

The first implementation should be Rust-first for all off-chain code.
There should be no TypeScript implementation in v1.
If bindings are needed later, they should wrap the Rust core.

## 4. Cryptographic Choices

## 4.1 Summary

Use the following primitives in v1:

- field for Cairo logic: Stark field `felt252`;
- hash inside proofs and Merkle trees: Poseidon builtin;
- on-chain tree hashing: Poseidon-compatible hash implementation matching the Cairo definition;
- nullifiers and commitments derived with Poseidon domain separation;
- PQ server signatures: hash-based XMSS-style signatures built from Poseidon-based WOTS+;
- homomorphic balance commitment: Pedersen commitment on the Stark curve;
- proof system: Cairo STARK proof system;
- Ethereum settlement asset: one ERC20 token per deployed vault.

## 4.2 Post-Quantum Requirement

The PQ requirement applies to:

- the proof system;
- the nullifier hash;
- the Merkle tree hash;
- the state-signature scheme;
- the mutual-close clearance signature.

The single accepted exception in v1 is:

- `E(B)` may use a Pedersen commitment on an elliptic curve because homomorphic addition and rerandomization are required.

That exception must be explicitly documented in code comments and in the README.

## 4.3 Domain Separation

Every Poseidon usage must be domain-separated.

Use the following deterministic conversion rule:

- `domain(label)` = the big-endian integer of the ASCII bytes of `label`, interpreted as a felt;
- all labels must be at most 31 bytes;
- labels are protocol constants committed into Rust, Cairo, and Solidity tests.

Use these labels:

- `zkapi.reg`
- `zkapi.leaf`
- `zkapi.node`
- `zkapi.null`
- `zkapi.state`
- `zkapi.clear`
- `zkapi.reqpub`
- `zkapi.wdpub`
- `zkapi.bal.g`
- `zkapi.bal.h`
- `zkapi.xmss.leaf`
- `zkapi.xmss.node`
- `zkapi.xmss.chain`
- `zkapi.xmss.msg`
- `zkapi.anchor`
- `zkapi.blind`

No unlabeled Poseidon invocation is allowed anywhere in the codebase.

## 4.4 Registration Commitment

The registration commitment is:

```text
C = Poseidon(domain("zkapi.reg"), s, 0)
```

Where:

- `s` is a random nonzero felt sampled uniformly mod the Stark field;
- `0` is a fixed version slot reserved for future extensions.

## 4.5 Active Note Leaf

The active-tree leaf is:

```text
leaf = Poseidon(domain("zkapi.leaf"), note_id, C, D, expiry_ts)
```

Where:

- `note_id` is the sequential on-chain note index;
- `C` is the registration commitment;
- `D` is the deposit amount in token base units as `u128`;
- `expiry_ts` is a Unix timestamp as `u64`.

The leaf is zeroed when:

- a mutual close succeeds;
- an escape-hatch withdrawal is initiated;
- an expired note is claimed.

If an escape-hatch withdrawal is successfully challenged, the original leaf is restored.

## 4.6 Merkle Tree

Use a fixed-depth binary Merkle tree with:

- depth `32`;
- leaf indices equal to `note_id`;
- zero leaf value `0`;
- internal node hash:

```text
node = Poseidon(domain("zkapi.node"), left, right)
```

The contract stores only the current root.

Every mutation of the tree takes a full sibling path as calldata and recomputes:

- the old root from `(index, old_leaf, siblings)`;
- the new root from `(index, new_leaf, siblings)`.

Deposits, withdrawals, challenge restores, and expiry claims all use the same Merkle update library.

## 4.7 Nullifiers

For both requests and withdrawals:

```text
x = Poseidon(domain("zkapi.null"), s, tau)
```

For the genesis state:

- `tau = 1`.

For later states:

- `tau` is the server-issued next anchor from the prior transition.

There is one nullifier namespace.
Ordinary requests and withdrawals share it.

## 4.8 Balance Commitment

Use a Pedersen commitment on the Stark curve:

```text
E(B, r) = B * G_balance + r * H_blind
```

Where:

- `B` is a `u128` amount;
- `r` is a random scalar mod the Stark-curve scalar field;
- `G_balance` and `H_blind` are fixed independent generators.

Implementation rule:

- derive `G_balance` and `H_blind` once with an offline hash-to-curve script using labels `zkapi.bal.g` and `zkapi.bal.h`;
- commit the resulting affine points into the repository as protocol constants;
- do not derive them at runtime.

The commitment must support:

- opening proof inside Cairo;
- rerandomization:

```text
E(B, r + rho) = E(B, r) + rho * H_blind
```

- server update by public charge:

```text
E(B - delta, r + rho + blind_delta_srv)
= E(B, r) + rho * H_blind - delta * G_balance + blind_delta_srv * H_blind
```

## 4.9 PQ State Signatures

Use a hash-based stateful signature scheme:

- WOTS+ over Poseidon as the one-time primitive;
- XMSS-style Merkle authentication tree over WOTS+ public keys;
- separate trees for state signatures and clearance signatures.

This is the required v1 key structure:

- `state_sig_root[epoch]`
- `clear_sig_root[epoch]`

Each root is published on-chain and mirrored off-chain.

Parameters for v1:

- `w = 16`;
- digest length `n = 248` bits represented inside one felt;
- `len1 = ceil(248 / 4) = 62`;
- `len2 = 3`;
- `len = 65`;
- XMSS tree height `20` for the initial implementation.

Each XMSS signature includes:

- `epoch: u32`
- `leaf_index: u32`
- `wots_sig[65]: felt252`
- `auth_path[20]: felt252`

State signature message:

```text
m_state = Poseidon(
  domain("zkapi.state"),
  protocol_version,
  chain_id,
  contract_address,
  E_x,
  E_y,
  tau
)
```

Clearance signature message:

```text
m_clear = Poseidon(
  domain("zkapi.clear"),
  protocol_version,
  chain_id,
  contract_address,
  withdrawal_nullifier
)
```

The state signature intentionally does not bind a stable note identifier.
This is an explicit privacy trade-off carried over from the protocol description.
Spending still requires:

- the secret `s`;
- the balance commitment opening;
- the valid signed state.

## 5. Data Model

## 5.1 On-Chain Note Metadata

The contract stores:

```text
struct Note {
  bytes32 commitment;        // C encoded as uint256 < field prime
  uint128 depositAmount;     // D
  uint64 expiryTs;           // T_exp
  Status status;             // Active, PendingWithdrawal, Closed
}
```

`Status` values:

- `0 = Uninitialized`
- `1 = Active`
- `2 = PendingWithdrawal`
- `3 = Closed`

The contract also stores pending escape-hatch state:

```text
struct PendingWithdrawal {
  bool exists;
  uint256 withdrawalNullifier;
  uint128 finalBalance;
  address destination;
  uint64 challengeDeadline;
}
```

Pending withdrawal data is keyed by `note_id`.

## 5.2 Client Private State

The client wallet must store:

```text
struct NoteState {
  uint16 protocolVersion;
  uint64 chainId;
  address contractAddress;
  uint32 noteId;
  felt252 secretS;
  uint128 depositAmount;
  uint64 expiryTs;
  uint128 currentBalance;
  felt252 balanceBlinding;
  CurvePoint currentCommitment;
  felt252 currentAnchor;             // 1 for genesis
  bool isGenesis;
  Option<u32> stateSigEpoch;
  Option<XmssSignature> stateSig;
}
```

The wallet must also store a write-ahead request journal:

```text
struct PendingRequestJournal {
  bool exists;
  uuid clientRequestId;
  felt252 nullifier;
  bytes32 payloadHash;
  uint64 createdAtMs;
}
```

The wallet uses the journal for recovery if it crashes after the server accepts a request but before the new state is persisted.

## 5.3 Server Transcript

The server must persist one durable transcript per consumed nullifier:

```text
struct ConsumedNullifier {
  felt252 nullifier;
  Status status;                     // Reserved or Finalized
  uuid clientRequestId;
  bytes32 payloadHash;
  RequestPublicInputs requestInputs;
  bytes requestProofBlob;
  uint128 chargeApplied;
  uint16 responseCode;
  bytes32 responseHash;
  CurvePoint nextCommitment;
  felt252 nextAnchor;
  felt252 blindDeltaSrv;
  u32 nextStateSigEpoch;
  XmssSignature nextStateSig;
  optional<uint32> policyReasonCode;
  optional<bytes32> policyEvidenceHash;
}
```

The server must retain:

- every spent nullifier forever or until an archival policy is introduced later;
- every request proof transcript at least until the note is closed or expired and the maximum challenge window has elapsed;
- in v1, the simplest acceptable rule is: retain all transcripts forever.

## 5.4 Public Input Structs

The shared Rust types and Solidity `Types.sol` file must define these structs exactly.

`RequestPublicInputs`:

```text
struct RequestPublicInputs {
  uint8 statementType;
  uint16 protocolVersion;
  uint64 chainId;
  address contractAddress;
  uint256 activeRoot;
  uint32 stateSigEpoch;
  uint256 stateSigRoot;
  uint256 requestNullifier;
  uint256 anonCommitmentX;
  uint256 anonCommitmentY;
  uint64 expiryTs;
  uint128 solvencyBound;
}
```

`WithdrawalPublicInputs`:

```text
struct WithdrawalPublicInputs {
  uint8 statementType;
  uint16 protocolVersion;
  uint64 chainId;
  address contractAddress;
  uint256 activeRoot;
  uint32 noteId;
  uint128 finalBalance;
  address destination;
  uint256 withdrawalNullifier;
  bool isGenesis;
  bool hasClearance;
  uint32 stateSigEpoch;
  uint256 stateSigRoot;
  uint32 clearSigEpoch;
  uint256 clearSigRoot;
}
```

The Cairo public outputs must map to these structs field-for-field.
The adapter must reject any proof whose decoded public outputs do not match the struct values exactly.
For request proofs, `stateSigEpoch == 0` and `stateSigRoot == 0` is the canonical encoding of the genesis path.
This is the request-proof equivalent of `isGenesis == true`.

## 6. Solidity Contract

## 6.1 Contract Name

The main contract is `ZkApiVault`.

One deployed contract instance manages:

- one ERC20 billing token;
- one operator treasury;
- one active-root tree;
- one set of server signing roots by epoch;
- one proof-adapter contract.

The contract must use:

- `ReentrancyGuard` or an equivalent non-reentrancy mechanism on every external state-mutating entrypoint;
- safe ERC20 transfer helpers;
- checks-effects-interactions ordering for all payout paths.

## 6.2 Immutable and Configurable Parameters

Immutable at deployment:

- `billingToken`
- `treasury`
- `noteTtl`
- `challengePeriod = 24 hours`
- `requestChargeCap`
- `policyChargeCap`
- `policyEnabled`
- `merkleDepth = 32`
- `protocolVersion = 1`

Owner-controlled, but rarely changed:

- proof adapter address;
- treasury address;
- active state-signing root epochs;
- pause flag.

The contract must be non-upgradeable in v1.

## 6.2.1 XMSS Epoch Lifecycle

XMSS roots are append-only in v1.

Rules:

- `rotateServerRoots` publishes a new epoch and makes it the epoch used for all newly issued state signatures and clearance signatures;
- old epochs remain valid for verification indefinitely in v1;
- the contract must reject re-registration of an existing epoch number;
- epoch numbers must increase strictly monotonically;
- the server must not issue signatures against an epoch until that epoch is visible on-chain;
- the server must rotate to a new epoch before exhausting the current XMSS tree;
- if the current signing tree is exhausted and no new epoch is published yet, the server must reject new work with `capacity_exhausted`;
- there is no protocol-level maximum number of retained verification epochs in v1;
- there is no revocation or deactivation flow in v1.

Rationale:

- previously issued states and clearances may need to be verified long after issuance;
- append-only epochs avoid breaking in-flight requests or already issued wallet states during rotation.

Client discovery rules:

- the server may advertise the latest epoch through an API or config endpoint;
- the client must treat on-chain epoch roots as authoritative;
- when a server response references an unknown epoch, the client must fetch that epoch root from chain or indexer before accepting the state.

Operational rule:

- in-flight requests are not invalidated by epoch rotation, because the request proof does not commit to the future epoch used for the returned next state;
- the returned next state may be signed under any on-chain published current epoch.

## 6.3 Public Functions

Required contract functions:

```text
deposit(bytes32 commitment, uint128 amount, bytes32[32] siblings)
mutualClose(WithdrawalPublicInputs inputs, bytes proofEnvelope, bytes32[32] siblings)
initiateEscapeWithdrawal(WithdrawalPublicInputs inputs, bytes proofEnvelope, bytes32[32] siblings)
challengeEscapeWithdrawal(uint32 noteId, RequestPublicInputs inputs, bytes proofEnvelope, bytes32[32] siblings)
finalizeEscapeWithdrawal(uint32 noteId)
claimExpired(uint32 noteId, bytes32[32] siblings)
setProofAdapter(address newAdapter)
setTreasury(address newTreasury)
rotateServerRoots(uint32 epoch, uint256 stateRoot, uint256 clearRoot)
pause()
unpause()
```

`deposit`:

- is `nonReentrant`;
- requires `amount > 0`;
- requires `commitment != 0`;
- requires `commitment < STARK_FIELD_PRIME`;
- transfers ERC20 from caller into the vault;
- allocates `note_id = nextNoteId`;
- computes `expiry = block.timestamp + noteTtl`;
- verifies current root contains zero leaf at `note_id`;
- sets leaf to the new active note leaf;
- stores note metadata;
- increments `nextNoteId`.

`mutualClose`:

- verifies withdrawal proof via adapter;
- is `nonReentrant`;
- requires `inputs.statementType == 2`;
- requires `inputs.hasClearance = true`;
- requires `inputs.activeRoot == currentRoot`;
- requires `stateSigRootByEpoch[inputs.stateSigEpoch] == inputs.stateSigRoot` when `inputs.isGenesis = false`;
- requires `clearSigRootByEpoch[inputs.clearSigEpoch] == inputs.clearSigRoot`;
- requires note status is `Active`;
- requires `inputs.finalBalance <= depositAmount`;
- zeroes the note leaf;
- marks note `Closed`;
- pays `finalBalance` to `destination`;
- pays `depositAmount - finalBalance` to `treasury`.

`initiateEscapeWithdrawal`:

- verifies withdrawal proof via adapter;
- is `nonReentrant`;
- requires `inputs.statementType == 2`;
- requires `inputs.hasClearance = false`;
- requires `inputs.activeRoot == currentRoot`;
- requires `stateSigRootByEpoch[inputs.stateSigEpoch] == inputs.stateSigRoot` when `inputs.isGenesis = false`;
- requires note status is `Active`;
- requires `inputs.finalBalance <= depositAmount`;
- zeroes the note leaf immediately to freeze the note;
- stores pending withdrawal data;
- marks note status `PendingWithdrawal`;
- sets `challengeDeadline = block.timestamp + challengePeriod`.

`challengeEscapeWithdrawal`:

- requires note status `PendingWithdrawal`;
- is `nonReentrant`;
- requires current time < `challengeDeadline`;
- verifies request proof via adapter;
- requires `inputs.statementType == 1`;
- requires `stateSigRootByEpoch[inputs.stateSigEpoch] == inputs.stateSigRoot` when `inputs.stateSigEpoch != 0`;
- requires `inputs.requestNullifier == pending.withdrawalNullifier`;
- requires the current root has zero at the note leaf index;
- restores the original leaf using the provided siblings;
- clears pending withdrawal data;
- marks note status `Active`.

`finalizeEscapeWithdrawal`:

- requires note status `PendingWithdrawal`;
- is `nonReentrant`;
- requires current time >= `challengeDeadline`;
- clears pending withdrawal data;
- marks note status `Closed`;
- pays user and treasury exactly as mutual close.

`claimExpired`:

- requires note status `Active`;
- is `nonReentrant`;
- requires `block.timestamp >= expiryTs`;
- zeroes the note leaf;
- marks note status `Closed`;
- transfers the full deposit amount to treasury.

Root freshness rule:

- v1 accepts only `currentRoot` for deposit, mutual close, escape withdrawal initiation, challenge restore, and expiry claim;
- any prior-root transaction that lands after another root-changing transaction must revert and be retried against the new root;
- this is an intentional simplicity trade-off in v1;
- accepting a bounded window of recent roots is a future optimization and out of scope for this spec.

Mutual-close retry rule:

- a root mismatch after clearance issuance does not consume the clearance proof path permanently;
- the user may fetch the new root and Merkle path, rebuild the withdrawal proof, and reuse the same clearance signature, because the clearance signature binds only the withdrawal nullifier and chain context, not the active root.

## 6.4 Events

Emit these events:

```text
event NoteDeposited(uint32 indexed noteId, bytes32 indexed commitment, uint128 amount, uint64 expiryTs, uint256 newRoot);
event MutualClose(uint32 indexed noteId, uint256 nullifier, uint128 finalBalance, address destination);
event EscapeWithdrawalInitiated(uint32 indexed noteId, uint256 nullifier, uint128 finalBalance, address destination, uint64 challengeDeadline, uint256 newRoot);
event EscapeWithdrawalChallenged(uint32 indexed noteId, uint256 nullifier, uint256 restoredRoot);
event EscapeWithdrawalFinalized(uint32 indexed noteId, uint256 nullifier, uint128 finalBalance, address destination);
event ExpiredClaimed(uint32 indexed noteId, uint128 depositAmount, uint256 newRoot);
event ServerRootsRotated(uint32 indexed epoch, uint256 stateRoot, uint256 clearRoot);
event ProofAdapterSet(address indexed newAdapter);
event TreasurySet(address indexed newTreasury);
```

## 6.5 Contract Invariants

The Solidity tests and invariant tests must enforce:

- an `Active` note leaf is present in the current Merkle root;
- a `PendingWithdrawal` note leaf is absent from the current root;
- a `Closed` note leaf is absent from the current root;
- vault token balance equals the sum of deposits of all non-closed notes plus any pending payouts not yet transferred in the current transaction;
- a challenged withdrawal restores exactly the original leaf;
- no function can pay out more than the original deposit amount.

## 7. Proof Adapter Boundary

## 7.1 Reason for the Adapter

The application contract must not depend directly on one specific Cairo-to-EVM verifier backend.

Use an adapter interface:

```solidity
interface IZkApiProofAdapter {
    function assertValidRequest(RequestPublicInputs calldata inputs, bytes calldata proofEnvelope) external view;
    function assertValidWithdrawal(WithdrawalPublicInputs calldata inputs, bytes calldata proofEnvelope) external view;
}
```

The adapter is responsible for:

- decoding the verifier-specific proof envelope;
- verifying an inline proof or checking a registered fact;
- pinning the allowed Cairo program hash for request proofs;
- pinning the allowed Cairo program hash for withdrawal proofs;
- checking the expected `statementType`:
  - `1` for `assertValidRequest`
  - `2` for `assertValidWithdrawal`
- rejecting mismatched public-input layouts.

## 7.2 Required Adapters

Implement two adapters:

1. `MockProofAdapter`
   For local contract tests.
   It should accept proofs signed by a test key or fixture hash and focus on business logic coverage.

2. `FactRegistryAdapter`
   Production adapter.
   It should follow the Cairo fact-registry pattern:
   - receive a proof envelope that references an already-verified fact;
   - reconstruct the expected public statement from the calldata struct;
   - assert that the configured fact registry marks that statement as valid.

This keeps the vault contract agnostic to whether the proof was:

- verified inline;
- verified elsewhere and registered as a fact;
- produced by a future Cairo verifier backend.

## 8. Cairo Programs

Implement two Cairo entrypoints:

- `request/program.cairo`
- `withdrawal/program.cairo`

Everything shared must live in library files under `cairo/src`.

## 8.1 Common Cairo Constraints

Both programs must enforce:

- `protocol_version == 1`;
- `contract_address` matches the target deployment;
- `chain_id` matches the target deployment;
- all felts that represent Ethereum addresses are `< 2^160`;
- all token amounts are valid `u128`;
- all Merkle paths have length `32`;
- all XMSS leaf indices are `< 2^20`;
- all elliptic-curve points are valid Stark-curve points;
- all selector bits are boolean.

## 8.2 Request Program

### Public Outputs

The request program must emit these public outputs in this exact order:

1. `statement_type = 1`
2. `protocol_version`
3. `chain_id`
4. `contract_address`
5. `active_root`
6. `state_sig_epoch`
7. `state_sig_root`
8. `request_nullifier`
9. `anon_commitment_x`
10. `anon_commitment_y`
11. `expiry_ts`
12. `solvency_bound`

`note_id` is intentionally omitted from request public outputs to preserve request-side privacy.
It is only revealed in the withdrawal program because on-chain settlement and Merkle leaf mutation require the note index.

### Private Witness

The request program witness must include:

- `secret_s`
- `note_id`
- `deposit_amount`
- `expiry_ts`
- `merkle_siblings[32]`
- `merkle_index_bits[32]`
- `current_balance`
- `current_blinding`
- `user_rerandomization`
- `current_anchor`
- `is_genesis`
- `state_sig_root`
- `state_sig_epoch`
- `state_sig` when `is_genesis = false`

### Request Constraints

The request program must prove:

1. `C = Poseidon(domain("zkapi.reg"), secret_s, 0)`
2. `leaf = Poseidon(domain("zkapi.leaf"), note_id, C, deposit_amount, expiry_ts)`
3. `leaf` is in `active_root`
4. if `is_genesis = 1`:
   - `current_anchor = 1`
   - `current_balance = deposit_amount`
   - `state_sig_epoch = 0`
   - `state_sig_root = 0`
   - no state signature is verified
5. if `is_genesis = 0`:
   - `E_current = Commit(current_balance, current_blinding)`
   - `state_sig_epoch > 0`
   - `state_sig_root != 0`
   - verify `state_sig` against `state_sig_root`
   - signed message is `m_state(E_current, current_anchor)`
6. `request_nullifier = Poseidon(domain("zkapi.null"), secret_s, current_anchor)`
7. `anon_commitment = Commit(current_balance, current_blinding + user_rerandomization)`
8. `current_balance >= solvency_bound`
9. `solvency_bound = requestChargeCap` when policy is disabled, else `solvency_bound = policyChargeCap`

The request program must not reveal:

- `secret_s`
- `note_id`
- `deposit_amount`
- `current_balance`
- `current_blinding`
- `current_anchor`

### Output Semantics

The server relies on:

- `request_nullifier` for replay protection;
- `anon_commitment` for homomorphic updating;
- `expiry_ts` for request-time expiry enforcement;
- `active_root` for latest-root enforcement.

## 8.3 Withdrawal Program

### Public Outputs

The withdrawal program must emit these public outputs in this exact order:

1. `statement_type = 2`
2. `protocol_version`
3. `chain_id`
4. `contract_address`
5. `active_root`
6. `note_id`
7. `final_balance`
8. `destination`
9. `withdrawal_nullifier`
10. `is_genesis`
11. `has_clearance`
12. `state_sig_epoch`
13. `state_sig_root`
14. `clear_sig_epoch`
15. `clear_sig_root`

### Private Witness

The withdrawal witness must include:

- `secret_s`
- `note_id`
- `deposit_amount`
- `expiry_ts`
- `merkle_siblings[32]`
- `merkle_index_bits[32]`
- `final_balance`
- `final_blinding`
- `current_anchor`
- `is_genesis`
- `state_sig_root`
- `state_sig_epoch`
- `state_sig` when `is_genesis = false`
- `has_clearance`
- `clear_sig_root`
- `clear_sig_epoch`
- `clear_sig` when `has_clearance = true`

### Withdrawal Constraints

The withdrawal program must prove:

1. `C = Poseidon(domain("zkapi.reg"), secret_s, 0)`
2. `leaf = Poseidon(domain("zkapi.leaf"), note_id, C, deposit_amount, expiry_ts)`
3. `leaf` is in `active_root`
4. if `is_genesis = 1`:
   - `current_anchor = 1`
   - `final_balance = deposit_amount`
   - `state_sig_epoch = 0`
   - `state_sig_root = 0`
   - no state signature is verified
5. if `is_genesis = 0`:
   - `E_current = Commit(final_balance, final_blinding)`
   - `state_sig_epoch > 0`
   - `state_sig_root != 0`
   - verify `state_sig` against `state_sig_root`
   - signed message is `m_state(E_current, current_anchor)`
6. `withdrawal_nullifier = Poseidon(domain("zkapi.null"), secret_s, current_anchor)`
7. `final_balance <= deposit_amount`
8. if `has_clearance = 1`:
   - `clear_sig_epoch > 0`
   - `clear_sig_root != 0`
   - verify `clear_sig` against `clear_sig_root`
   - signed message is `m_clear(withdrawal_nullifier)`
9. if `has_clearance = 0`:
   - `clear_sig_epoch = 0`
   - `clear_sig_root = 0`

The withdrawal program binds:

- the final revealed balance;
- the destination address;
- whether a server clearance exists.

This prevents front-running replacement of `destination`.

## 8.4 Challenge Proof Rule

The contract challenge path must accept an archived request proof only.

A server-signed receipt alone is not sufficient challenge evidence.

Reason:

- if receipt-only challenges were allowed, the server could fabricate a fake receipt after seeing a fallback withdrawal nullifier and block withdrawal maliciously.

Therefore challenge evidence must be:

- a real request proof that was previously generated by the user;
- or a verifier-backed fact corresponding to such a proof.

## 9. Server

## 9.1 Responsibilities

The server is responsible for:

- verifying request proofs;
- rejecting replayed nullifiers;
- executing or refusing the actual API request;
- computing the actual charge;
- deriving the next anchor;
- applying the homomorphic balance update;
- signing the next private state;
- persisting the transition transcript;
- serving crash recovery;
- serving withdrawal clearance signatures;
- monitoring pending escape withdrawals and challenging stale ones.

## 9.2 Request Verification Rules

For every request, the server must check:

1. `protocol_version` matches deployment config
2. `chain_id` and `contract_address` match deployment config
3. `active_root` equals the latest root from the chain/indexer
4. `expiry_ts > current_unix_time`
5. `solvency_bound` matches server config:
   - `requestChargeCap` if policy disabled
   - `policyChargeCap` if policy enabled
6. `statementType == 1`
7. if `state_sig_epoch != 0`, `state_sig_root` equals the configured root for that epoch
8. if `state_sig_epoch == 0`, `state_sig_root == 0`
9. the Cairo request proof verifies
10. `request_nullifier` is absent from `spent_nullifiers`

The server must reject stale-root proofs even if the proof itself is valid.

## 9.3 Request Processing Algorithm

Use this exact high-level order:

1. parse request and canonicalize fields
2. verify proof and static inputs
3. insert `request_nullifier` into DB as `Reserved`
4. if insert fails because the nullifier already exists:
   - if `clientRequestId` and `payloadHash` match an existing finalized record, return the stored result
   - otherwise reject as replay
5. execute the provider call
6. compute `chargeApplied`
7. enforce:
   - `chargeApplied >= 0`
   - `chargeApplied <= requestChargeCap` for ordinary requests
   - `chargeApplied <= policyChargeCap` for policy rejections
8. compute `nextBalance = currentBalance - chargeApplied` on the client side only; the server never learns `currentBalance`
9. derive `nextAnchor`
10. derive `blindDeltaSrv`
11. compute:

```text
nextCommitment = anonCommitment - chargeApplied * G_balance + blindDeltaSrv * H_blind
```

12. sign `m_state(nextCommitment, nextAnchor)` with the current state-signing XMSS tree
13. persist the full finalized transcript atomically
14. return the response payload plus next state

If the provider rejects on policy grounds:

- the server may apply a charge up to `policyChargeCap`;
- it must still consume the state and return a valid next state.

If the request fails due to the server's own fault:

- the server must charge `0`;
- it must still return or later recover a valid next state so the user is not stuck.

Provider idempotency rule:

- every provider execution must be keyed by `clientRequestId`;
- the server must pass `clientRequestId` through as an idempotency key whenever the downstream API supports one;
- if the downstream API does not support idempotency, that integration is not compliant with v1 for billable mutations.

## 9.4 Next Anchor Derivation

The next anchor must be unpredictable and unique.

Derive it as:

```text
nextAnchor = Poseidon(
  domain("zkapi.anchor"),
  server_rng_output,
  request_nullifier,
  nextCommitment_x,
  nextCommitment_y,
  state_sig_leaf_index
)
```

The server must reject `nextAnchor = 0`.
If zero occurs, draw fresh randomness and retry.

## 9.5 Blind Delta Derivation

Derive `blindDeltaSrv` as:

```text
blindDeltaSrv = Poseidon(
  domain("zkapi.blind"),
  server_rng_output_2,
  request_nullifier,
  state_sig_leaf_index
) mod curve_order
```

Zero is allowed.

## 9.6 Server Response Format

Successful request responses must include:

```text
{
  status: "ok",
  client_request_id,
  request_nullifier,
  response_code,
  response_payload,
  response_hash,
  charge_applied,
  next_commitment: { x, y },
  next_anchor,
  blind_delta_srv,
  next_state_sig_epoch,
  next_state_sig,
  optional policy_reason_code,
  optional policy_evidence_hash
}
```

The client must verify:

- the algebra of `next_commitment`;
- the XMSS signature on the returned state;
- the charge bound;
- the protocol version and chain context.

Error responses must use this envelope:

```text
{
  status: "error",
  client_request_id,
  error_code,
  error_message,
  retriable,
  optional latest_root,
  optional server_time_ms
}
```

Required error codes:

- `invalid_proof`
- `stale_root`
- `replay`
- `note_expired`
- `internal_error`
- `capacity_exhausted`

Error rules:

- `stale_root` must include `latest_root`;
- `replay` must set `retriable = false`;
- `internal_error` should set `retriable = true` unless the server knows the note is permanently stuck;
- if a finalized transcript already exists for the same `(clientRequestId, payloadHash)`, the server should return the stored success response instead of an error.

## 9.7 Clearance API

For mutual close, the server exposes a clearance endpoint:

```text
POST /v1/withdraw/clearance
{
  withdrawal_nullifier
}
```

The server:

1. checks whether `withdrawal_nullifier` already exists in `spent_nullifiers`
2. if used, rejects
3. if clean, signs `m_clear(withdrawal_nullifier)` using the clearance XMSS tree
4. marks the nullifier as spent in the same table with a dedicated `clearance_reserved` status

If the user never submits the mutual close after clearance was issued, that nullifier is still considered consumed.
This is intentional and matches the protocol model.

## 9.8 Recovery API

The server must expose:

```text
GET /v1/requests/{client_request_id}
GET /v1/nullifiers/{request_nullifier}
```

Both endpoints return:

- current processing status;
- stored transition result if finalized.

The client uses these endpoints after a crash or network timeout.

Reserved-state recovery rule:

- on restart, the server must scan all nullifiers still marked `Reserved`;
- for each reserved entry, the server must use `clientRequestId` to query or replay the downstream provider idempotently;
- if the downstream outcome is known, the server must finalize the original transcript deterministically;
- if the downstream outcome remains unknown beyond an operator-configured recovery timeout, the server must finalize the request with:
  - `chargeApplied = 0`
  - an `internal_error` response
  - a valid next state transition

This prevents `Reserved` entries from remaining stuck indefinitely.

## 9.9 Challenge Watcher

The server must run a watcher that listens for:

- `EscapeWithdrawalInitiated`

For each pending withdrawal:

1. look up `withdrawal_nullifier` in the transcript store
2. if not found, do nothing
3. if found, submit `challengeEscapeWithdrawal` with:
   - the stored request public inputs
   - the stored request proof or fact reference
   - the current zero-leaf sibling path for `note_id`

The watcher must retry until:

- the challenge succeeds;
- the deadline passes;
- or the withdrawal finalizes or is already challenged.

## 10. Client SDK

## 10.1 Responsibilities

The client SDK is responsible for:

- generating `secret_s`;
- building deposits and note state;
- tracking the latest root and sibling path;
- fetching and validating server XMSS roots against on-chain state;
- generating Cairo request proofs;
- generating Cairo withdrawal proofs;
- verifying server responses;
- persisting the next private state atomically;
- recovering unfinished requests;
- requesting mutual-close clearance.

## 10.2 Deposit Flow

The deposit flow is:

1. sample `secret_s`
2. compute `C`
3. fetch latest root and zero-leaf path for `nextNoteId`
4. call `deposit`
5. persist wallet state:
   - `currentBalance = depositAmount`
   - `balanceBlinding = r0`
   - `currentCommitment = Commit(depositAmount, r0)`
   - `currentAnchor = 1`
   - `isGenesis = true`
   - `stateSig = None`

`r0` must be sampled client-side at deposit time and never shared.

## 10.3 Request Flow

The request flow is:

1. sync latest root and Merkle path
2. build request public inputs and witness
3. choose `user_rerandomization`
4. compute `anonCommitment`
5. generate request proof
6. write `PendingRequestJournal`
7. call server `/v1/requests`
8. verify server response:
   - `chargeApplied <= configured cap`
   - `nextCommitment` algebra is correct
   - the referenced `next_state_sig_epoch` exists on-chain
   - returned state signature verifies
9. compute:

```text
nextBalance = currentBalance - chargeApplied
nextBlinding = currentBlinding + user_rerandomization + blindDeltaSrv mod curve_order
```

10. atomically replace wallet state with the returned next state
11. clear the request journal

## 10.4 Request Recovery

On startup, if `PendingRequestJournal.exists = true`, the client must:

1. query the server recovery endpoint
2. if finalized, verify and install the returned next state
3. if still reserved, keep polling with backoff
4. if unknown, surface an unrecoverable operator error and stop further spending from that note until resolved

The client must never blindly retry the same logical request with a new proof if a pending journal exists.
If the original failure was `stale_root`, the client may discard the failed proof, refresh the root and path, and generate a fresh proof because the nullifier was never reserved successfully.

## 10.5 Withdrawal Flow

Mutual-close flow:

1. compute `withdrawal_nullifier`
2. request clearance signature from server
3. sync latest root and Merkle path
4. build withdrawal proof with `has_clearance = true`
5. submit `mutualClose`

If `mutualClose` reverts due to a root mismatch:

1. keep the same clearance signature
2. refresh the latest root and Merkle path
3. rebuild the withdrawal proof
4. retry `mutualClose`

Escape-hatch flow:

1. compute `withdrawal_nullifier`
2. sync latest root and Merkle path
3. build withdrawal proof with `has_clearance = false`
4. submit `initiateEscapeWithdrawal`
5. wait for either:
   - `EscapeWithdrawalChallenged`
   - or challenge deadline and then call `finalizeEscapeWithdrawal`

After any successful close, the client must archive and then delete the note's private state.

## 11. Indexer

## 11.1 Requirement

An indexer or local tree mirror is required in v1.
The contract stores only the current root, not the full tree.

The indexer must:

- consume contract events in block order;
- maintain the current leaf map by `note_id`;
- maintain the current root;
- serve sibling paths for:
  - zero leaf at `note_id`
  - active note leaf at `note_id`

The indexer is not trusted.
Incorrect sibling paths only cause proof or transaction failure.

## 11.2 Required Endpoints

At minimum, the indexer should expose:

```text
GET /v1/tree/root
GET /v1/tree/notes/{note_id}/path
GET /v1/tree/notes/{note_id}/zero-path
GET /v1/tree/next-note-id
```

The client and server may also embed this logic locally if they consume the same chain events.

## 12. Wire Formats and Serialization

## 12.1 JSON Rules

For HTTP APIs:

- field elements are `0x`-prefixed lowercase hex strings;
- curve points are objects with `x` and `y` hex fields;
- `u128`, `u64`, and `u32` values are decimal strings in JSON;
- proof blobs are base64 strings;
- UUIDs use canonical textual form.

## 12.2 Solidity Rules

For Solidity:

- all felts are passed as `uint256`;
- the contract must reject any felt `>= STARK_FIELD_PRIME` where relevant;
- addresses are normal `address`;
- amounts are `uint128`;
- timestamps are `uint64`;
- `note_id` is `uint32`.

## 12.3 Proof Envelope

The proof adapter must accept an opaque `bytes proofEnvelope`.

Supported envelope kinds:

- `0x01 || fact_reference_payload`
- `0x02 || inline_proof_payload`

The vault contract does not inspect the envelope beyond passing it to the adapter.

## 13. Testing Requirements

## 13.1 Cairo Tests

Must cover:

- domain-separated Poseidon helpers;
- Merkle membership verification;
- Pedersen opening and rerandomization checks;
- WOTS+ chain verification;
- XMSS auth-path verification;
- request proof happy path;
- request proof stale root failure;
- request proof insufficient balance failure;
- withdrawal proof with state signature;
- withdrawal proof genesis path;
- withdrawal proof mutual-close path;
- withdrawal proof bad clearance failure.

## 13.2 Solidity Tests

Must cover:

- deposit creates an active note and updates the root;
- mutual close removes the leaf and settles correctly;
- escape-hatch initiation freezes the note immediately;
- challenge restores the leaf exactly;
- finalize after deadline settles correctly;
- claimExpired transfers full deposit to treasury;
- invalid proof rejection;
- invalid siblings rejection;
- double finalize rejection;
- challenge after deadline rejection;
- root mismatch rejection.

## 13.3 End-to-End Integration Tests

Must cover:

1. deposit -> request -> request -> mutual close
2. deposit -> request -> escape withdrawal -> challenge -> valid withdrawal with latest state
3. deposit -> no spends -> claimExpired
4. deposit -> request -> client crash -> recovery endpoint -> continue spending
5. deposit -> request while unrelated deposit changes root -> server returns stale-root error -> client reproves successfully
6. policy rejection path with bounded penalty

## 13.4 Property Tests

Must cover:

- homomorphic commitment update algebra;
- nullifier uniqueness over random anchors;
- Merkle update then restore returns the prior root;
- no payout path exceeds original deposit;
- XMSS leaf index is never reused under concurrent signing.

## 14. Security Rules

The implementation must enforce these rules explicitly:

- latest active root only for request proofs;
- external state-mutating Solidity entrypoints are non-reentrant;
- ERC20 transfers use safe wrappers and follow checks-effects-interactions ordering;
- withdrawal initiation freezes the note immediately;
- stale-withdrawal challenge requires a real archived request proof;
- no server receipt-only challenge;
- client state persistence is atomic;
- every consumed nullifier results in exactly one finalized transcript;
- XMSS leaf indices are allocated under a DB transaction or equivalent lock;
- no unlabeled hash invocation;
- no direct ECDSA/ECDH-based security assumption in the core protocol path;
- the Pedersen balance commitment exception is isolated to one module.

## 15. Implementation Order

Build in this order:

1. shared Rust types, domain constants, serialization
2. Merkle library in Rust, Cairo, and Solidity tests
3. Pedersen balance commitment module and algebra tests
4. XMSS/WOTS+ verifier in Cairo and Rust
5. request Cairo program and off-chain verifier path
6. withdrawal Cairo program and off-chain verifier path
7. Solidity vault with `MockProofAdapter`
8. Rust client state machine and recovery journal
9. Rust server nullifier store, signer, and request pipeline
10. end-to-end tests against mock adapter
11. production `FactRegistryAdapter`
12. pending-withdrawal challenge watcher

Do not start with frontend or deployment scripts.
The protocol-critical core must be complete before any UX work.

## 16. Definition of Done

The implementation is complete only when all of the following are true:

- both Cairo programs compile and produce stable public-output layouts;
- the Rust client can deposit, generate proofs, update state, recover from crashes, and withdraw;
- the Rust server can verify proofs, prevent replay, settle charges, sign next states, and challenge stale withdrawals;
- the Solidity contract passes its full Foundry suite;
- the end-to-end integration tests pass with at least one real Cairo proof backend and one mock adapter path;
- all cryptographic constants are committed and cross-checked across Rust, Cairo, and Solidity tests;
- the README explains the PQ model and the Pedersen exception clearly.

## 17. Explicit Clarifications Relative to `PROTOCOL.md`

These points are required implementation clarifications, not optional changes:

- the on-chain note tree is mutable and tracks currently active notes, not historical membership only;
- escape-hatch initiation removes the note leaf immediately so the note is frozen during the challenge window;
- a successful challenge restores the leaf;
- the client must track the balance blinding factor, and the server must return `blindDeltaSrv`;
- the server must retain request proofs because stale-withdrawal challenges rely on them;
- request proofs use the latest active root only;
- request proofs encode genesis as `stateSigEpoch = 0` and `stateSigRoot = 0`;
- withdrawal proofs bind the destination address to prevent front-running;
- clearance signatures are verified inside the Cairo withdrawal proof, not directly in Solidity.
