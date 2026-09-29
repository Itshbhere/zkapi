// Withdrawal proof private witness (spec section 8.3).
//
// These fields are known only to the prover and are never revealed publicly.

/// Private witness data for the withdrawal proof program.
#[derive(Drop, Copy)]
pub struct WithdrawalWitness {
    /// User secret (random nonzero felt).
    pub secret_s: felt252,
    /// On-chain note index.
    pub note_id: felt252,
    /// Original deposit amount (u128 encoded as felt).
    pub deposit_amount: felt252,
    /// Note expiry Unix timestamp.
    pub expiry_ts: felt252,
    /// Final balance to withdraw (u128 encoded as felt).
    pub final_balance: felt252,
    /// Blinding factor for the final balance commitment.
    pub final_blinding: felt252,
    /// Current anchor (1 for genesis, server-issued otherwise).
    pub current_anchor: felt252,
    /// 1 if this is the genesis (first) state, 0 otherwise.
    pub is_genesis: felt252,
    /// XMSS root of the state signature tree (0 for genesis).
    pub state_sig_root: felt252,
    /// XMSS epoch of the state signature (0 for genesis).
    pub state_sig_epoch: u32,
    /// 1 if the server issued a clearance signature, 0 otherwise.
    pub has_clearance: felt252,
    /// XMSS root of the clearance signature tree (0 if no clearance).
    pub clear_sig_root: felt252,
    /// XMSS epoch of the clearance signature (0 if no clearance).
    pub clear_sig_epoch: u32,
}
