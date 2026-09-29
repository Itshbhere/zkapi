// Withdrawal proof public inputs (spec section 8.3).
//
// These are the values that are publicly visible in the proof output.
// They map field-for-field to the Solidity `WithdrawalPublicInputs` struct.

/// Public outputs of the withdrawal proof program, emitted in this exact order.
#[derive(Drop, Copy)]
pub struct WithdrawalPublicInputs {
    /// Always 2 (STATEMENT_TYPE_WITHDRAWAL).
    pub statement_type: felt252,
    /// Protocol version (must be 1 in v1).
    pub protocol_version: felt252,
    /// Target chain id.
    pub chain_id: felt252,
    /// Target contract address (< 2^160).
    pub contract_address: felt252,
    /// Current active Merkle root.
    pub active_root: felt252,
    /// On-chain note index (revealed for Merkle leaf mutation).
    pub note_id: felt252,
    /// Final balance to pay out (must be <= deposit_amount).
    pub final_balance: felt252,
    /// EVM destination address for the payout (< 2^160).
    pub destination: felt252,
    /// Nullifier for replay protection.
    pub withdrawal_nullifier: felt252,
    /// 1 if this is the genesis state, 0 otherwise (encoded as felt).
    pub is_genesis: felt252,
    /// 1 if the server issued a clearance signature, 0 otherwise.
    pub has_clearance: felt252,
    /// XMSS epoch of the state signature (0 for genesis).
    pub state_sig_epoch: u32,
    /// XMSS root of the state signature tree (0 for genesis).
    pub state_sig_root: felt252,
    /// XMSS epoch of the clearance signature (0 if no clearance).
    pub clear_sig_epoch: u32,
    /// XMSS root of the clearance signature tree (0 if no clearance).
    pub clear_sig_root: felt252,
}
