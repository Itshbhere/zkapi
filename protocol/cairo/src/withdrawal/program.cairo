// Withdrawal proof program (spec section 8.3).
//
// This module implements the main proving logic for a withdrawal proof.
// The prover calls `run_withdrawal_program` with all public and private
// inputs; the function asserts every constraint and returns the public
// outputs as a serialized array of felt252 values.

use core::poseidon::poseidon_hash_span;
use zkapi_cairo::constants::{GENESIS_ANCHOR, PROTOCOL_VERSION, STATEMENT_TYPE_WITHDRAWAL};
use zkapi_cairo::domains::{DOMAIN_CLEAR, DOMAIN_LEAF, DOMAIN_NULL, DOMAIN_REG, DOMAIN_STATE};
use zkapi_cairo::merkle::verify_merkle_path;
use zkapi_cairo::pedersen_balance::compute_commitment;
use zkapi_cairo::xmss::verify::verify_xmss;

/// Execute the withdrawal proof program.
///
/// All inputs are passed as explicit arguments.  The function panics if any
/// constraint is violated and returns an array of public outputs on success.
///
/// Public outputs are emitted in the exact order defined by
/// `WithdrawalPublicInputs` (spec section 8.3):
///   1.  statement_type = 2
///   2.  protocol_version
///   3.  chain_id
///   4.  contract_address
///   5.  active_root
///   6.  note_id
///   7.  final_balance
///   8.  destination
///   9.  withdrawal_nullifier
///  10.  is_genesis
///  11.  has_clearance
///  12.  state_sig_epoch
///  13.  state_sig_root
///  14.  clear_sig_epoch
///  15.  clear_sig_root
pub fn run_withdrawal_program(
    // -- context (public) --
    protocol_version: felt252,
    chain_id: felt252,
    contract_address: felt252,
    active_root: felt252,
    destination: felt252,
    // -- private witness --
    secret_s: felt252,
    note_id: felt252,
    deposit_amount: felt252,
    expiry_ts: felt252,
    merkle_index_bits: Span<felt252>,
    merkle_siblings: Span<felt252>,
    final_balance: felt252,
    final_blinding: felt252,
    current_anchor: felt252,
    is_genesis: felt252,
    state_sig_root: felt252,
    state_sig_epoch: u32,
    state_sig_leaf_index: u32,
    // XMSS state-signature components (ignored when is_genesis == 1)
    state_wots_sig: Span<felt252>,
    state_auth_path: Span<felt252>,
    // Clearance fields
    has_clearance: felt252,
    clear_sig_root: felt252,
    clear_sig_epoch: u32,
    clear_sig_leaf_index: u32,
    // XMSS clearance-signature components (ignored when has_clearance == 0)
    clear_wots_sig: Span<felt252>,
    clear_auth_path: Span<felt252>,
) -> Array<felt252> {
    // ---------------------------------------------------------------
    // 0. Protocol version check
    // ---------------------------------------------------------------
    assert(protocol_version == PROTOCOL_VERSION, 'bad protocol version');

    // ---------------------------------------------------------------
    // 1. Registration commitment
    //    C = Poseidon(DOMAIN_REG, secret_s, 0)
    // ---------------------------------------------------------------
    assert(secret_s != 0, 'secret_s must be nonzero');
    let c = poseidon_hash_span(array![DOMAIN_REG, secret_s, 0].span());

    // ---------------------------------------------------------------
    // 2. Note leaf
    //    leaf = Poseidon(DOMAIN_LEAF, note_id, C, deposit_amount, expiry_ts)
    // ---------------------------------------------------------------
    let leaf = poseidon_hash_span(
        array![DOMAIN_LEAF, note_id, c, deposit_amount, expiry_ts].span(),
    );

    // ---------------------------------------------------------------
    // 3. Verify leaf is in active_root
    // ---------------------------------------------------------------
    verify_merkle_path(active_root, leaf, merkle_index_bits, merkle_siblings);

    // ---------------------------------------------------------------
    // 4 / 5. Genesis vs. signed state
    // ---------------------------------------------------------------
    assert(is_genesis == 0 || is_genesis == 1, 'is_genesis must be bool');

    if is_genesis == 1 {
        // Genesis path
        assert(current_anchor == GENESIS_ANCHOR, 'genesis anchor must be 1');
        assert(final_balance == deposit_amount, 'genesis balance != deposit');
        assert(state_sig_epoch == 0, 'genesis epoch must be 0');
        assert(state_sig_root == 0, 'genesis sig root must be 0');
    } else {
        // Non-genesis: verify state signature
        let (e_x, e_y) = compute_commitment(final_balance, final_blinding);

        assert(state_sig_epoch != 0, 'non-genesis epoch must be > 0');
        assert(state_sig_root != 0, 'non-genesis sig root != 0');

        // m_state = Poseidon(DOMAIN_STATE, protocol_version, chain_id,
        //                    contract_address, E_x, E_y, current_anchor)
        let m_state = poseidon_hash_span(
            array![
                DOMAIN_STATE, protocol_version, chain_id, contract_address, e_x, e_y,
                current_anchor,
            ]
                .span(),
        );

        verify_xmss(state_sig_root, m_state, state_sig_leaf_index, state_wots_sig, state_auth_path);
    }

    // ---------------------------------------------------------------
    // 6. Withdrawal nullifier
    //    withdrawal_nullifier = Poseidon(DOMAIN_NULL, secret_s, current_anchor)
    // ---------------------------------------------------------------
    let withdrawal_nullifier = poseidon_hash_span(
        array![DOMAIN_NULL, secret_s, current_anchor].span(),
    );

    // ---------------------------------------------------------------
    // 7. final_balance <= deposit_amount
    // ---------------------------------------------------------------
    let final_u128: u128 = final_balance.try_into().expect('final_balance not u128');
    let deposit_u128: u128 = deposit_amount.try_into().expect('deposit_amount not u128');
    assert(final_u128 <= deposit_u128, 'final > deposit');

    // ---------------------------------------------------------------
    // 8 / 9. Clearance signature
    // ---------------------------------------------------------------
    assert(has_clearance == 0 || has_clearance == 1, 'has_clearance must be bool');

    if has_clearance == 1 {
        assert(clear_sig_epoch != 0, 'clearance epoch must be > 0');
        assert(clear_sig_root != 0, 'clearance sig root != 0');

        // m_clear = Poseidon(DOMAIN_CLEAR, protocol_version, chain_id,
        //                    contract_address, withdrawal_nullifier)
        let m_clear = poseidon_hash_span(
            array![DOMAIN_CLEAR, protocol_version, chain_id, contract_address, withdrawal_nullifier]
                .span(),
        );

        verify_xmss(clear_sig_root, m_clear, clear_sig_leaf_index, clear_wots_sig, clear_auth_path);
    } else {
        assert(clear_sig_epoch == 0, 'no-clear epoch must be 0');
        assert(clear_sig_root == 0, 'no-clear sig root must be 0');
    }

    // ---------------------------------------------------------------
    // 10. Emit public outputs
    // ---------------------------------------------------------------
    let state_sig_epoch_felt: felt252 = state_sig_epoch.into();
    let clear_sig_epoch_felt: felt252 = clear_sig_epoch.into();

    array![
        STATEMENT_TYPE_WITHDRAWAL, protocol_version, chain_id, contract_address, active_root,
        note_id, final_balance, destination, withdrawal_nullifier, is_genesis, has_clearance,
        state_sig_epoch_felt, state_sig_root, clear_sig_epoch_felt, clear_sig_root,
    ]
}

#[cfg(test)]
mod tests {
    use core::poseidon::poseidon_hash_span;
    use zkapi_cairo::constants::MERKLE_DEPTH;
    use zkapi_cairo::domains::{DOMAIN_LEAF, DOMAIN_NODE, DOMAIN_REG};
    use super::run_withdrawal_program;

    fn zero_path_for_leaf(leaf: felt252) -> (Array<felt252>, Array<felt252>, felt252) {
        let mut index_bits = array![];
        let mut siblings = array![];
        let mut zero = 0;
        let mut current = leaf;
        let mut i: u32 = 0;
        while i < MERKLE_DEPTH {
            index_bits.append(0);
            siblings.append(zero);
            current = poseidon_hash_span(array![DOMAIN_NODE, current, zero].span());
            zero = poseidon_hash_span(array![DOMAIN_NODE, zero, zero].span());
            i += 1;
        }
        (index_bits, siblings, current)
    }

    #[test]
    fn test_run_withdrawal_program_genesis_escape() {
        let secret_s = 42;
        let note_id = 0;
        let deposit_amount = 1000;
        let expiry_ts = 1700000000;
        let commitment = poseidon_hash_span(array![DOMAIN_REG, secret_s, 0].span());
        let leaf = poseidon_hash_span(
            array![DOMAIN_LEAF, note_id, commitment, deposit_amount, expiry_ts].span(),
        );
        let (index_bits, siblings, active_root) = zero_path_for_leaf(leaf);

        let outputs = run_withdrawal_program(
            1,
            1,
            0xdead,
            active_root,
            0xbeef,
            secret_s,
            note_id,
            deposit_amount,
            expiry_ts,
            index_bits.span(),
            siblings.span(),
            deposit_amount,
            0,
            1,
            1,
            0,
            0,
            0,
            array![].span(),
            array![].span(),
            0,
            0,
            0,
            0,
            array![].span(),
            array![].span(),
        );

        let outputs_span = outputs.span();
        assert(*outputs_span.at(0) == 2, 'statement type');
        assert(*outputs_span.at(4) == active_root, 'root mismatch');
        assert(*outputs_span.at(6) == deposit_amount, 'final balance mismatch');
        assert(*outputs_span.at(9) == 1, 'is_genesis mismatch');
        assert(*outputs_span.at(10) == 0, 'has_clearance mismatch');
    }
}
