// Request proof public inputs (spec section 8.2).
//
// These are the values that are publicly visible in the proof output.
// They map field-for-field to the Solidity `RequestPublicInputs` struct.

/// Public outputs of the request proof program, emitted in this exact order.
#[derive(Drop, Copy)]
pub struct RequestPublicInputs {
    /// Always 1 (STATEMENT_TYPE_REQUEST).
    pub statement_type: felt252,
    /// Protocol version (must be 1 in v1).
    pub protocol_version: felt252,
    /// Target chain id.
    pub chain_id: felt252,
    /// Target contract address (< 2^160).
    pub contract_address: felt252,
    /// Current active Merkle root.
    pub active_root: felt252,
    /// XMSS epoch of the state signature (0 for genesis).
    pub state_sig_epoch: u32,
    /// XMSS root of the state signature tree (0 for genesis).
    pub state_sig_root: felt252,
    /// Nullifier for replay protection.
    pub request_nullifier: felt252,
    /// Rerandomized balance commitment x-coordinate.
    pub anon_commitment_x: felt252,
    /// Rerandomized balance commitment y-coordinate.
    pub anon_commitment_y: felt252,
    /// Note expiry timestamp.
    pub expiry_ts: felt252,
    /// Minimum balance the prover guarantees.
    pub solvency_bound: felt252,
}
