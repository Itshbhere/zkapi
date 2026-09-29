//! State signature message computation.
//!
//! m_state = Poseidon(domain("zkapi.state"), protocol_version, chain_id, contract_address, E_x, E_y, tau)
//! m_clear = Poseidon(domain("zkapi.clear"), protocol_version, chain_id, contract_address, withdrawal_nullifier)

use zkapi_types::domain::{DOMAIN_CLEAR, DOMAIN_STATE};
use zkapi_types::Felt252;

use crate::poseidon::poseidon_hash_chain;

/// Compute the state signature message.
pub fn compute_state_message(
    protocol_version: u16,
    chain_id: u64,
    contract_address: &Felt252,
    commitment_x: &Felt252,
    commitment_y: &Felt252,
    anchor: &Felt252,
) -> Felt252 {
    poseidon_hash_chain(
        &DOMAIN_STATE,
        &[
            Felt252::from_u64(protocol_version as u64),
            Felt252::from_u64(chain_id),
            *contract_address,
            *commitment_x,
            *commitment_y,
            *anchor,
        ],
    )
}

/// Compute the clearance signature message.
pub fn compute_clearance_message(
    protocol_version: u16,
    chain_id: u64,
    contract_address: &Felt252,
    withdrawal_nullifier: &Felt252,
) -> Felt252 {
    poseidon_hash_chain(
        &DOMAIN_CLEAR,
        &[
            Felt252::from_u64(protocol_version as u64),
            Felt252::from_u64(chain_id),
            *contract_address,
            *withdrawal_nullifier,
        ],
    )
}

/// Compute the next anchor.
///
/// nextAnchor = Poseidon(domain("zkapi.anchor"), rng, nullifier, commitment_x, commitment_y, sig_leaf_index)
pub fn compute_next_anchor(
    server_rng: &Felt252,
    request_nullifier: &Felt252,
    next_commitment_x: &Felt252,
    next_commitment_y: &Felt252,
    state_sig_leaf_index: u32,
) -> Felt252 {
    use zkapi_types::domain::DOMAIN_ANCHOR;
    poseidon_hash_chain(
        &DOMAIN_ANCHOR,
        &[
            *server_rng,
            *request_nullifier,
            *next_commitment_x,
            *next_commitment_y,
            Felt252::from_u64(state_sig_leaf_index as u64),
        ],
    )
}

/// Compute the blind delta for the server.
///
/// blindDeltaSrv = Poseidon(domain("zkapi.blind"), rng2, nullifier, sig_leaf_index) mod curve_order
pub fn compute_blind_delta(
    server_rng: &Felt252,
    request_nullifier: &Felt252,
    state_sig_leaf_index: u32,
) -> Felt252 {
    use zkapi_types::domain::DOMAIN_BLIND;
    poseidon_hash_chain(
        &DOMAIN_BLIND,
        &[
            *server_rng,
            *request_nullifier,
            Felt252::from_u64(state_sig_leaf_index as u64),
        ],
    )
    // Note: caller must reduce mod curve_order for EC operations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_message_deterministic() {
        let addr = Felt252::from_u64(0xdeadbeef);
        let cx = Felt252::from_u64(1);
        let cy = Felt252::from_u64(2);
        let anchor = Felt252::from_u64(100);
        let m1 = compute_state_message(1, 1, &addr, &cx, &cy, &anchor);
        let m2 = compute_state_message(1, 1, &addr, &cx, &cy, &anchor);
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_clearance_message_deterministic() {
        let addr = Felt252::from_u64(0xdeadbeef);
        let null = Felt252::from_u64(999);
        let m1 = compute_clearance_message(1, 1, &addr, &null);
        let m2 = compute_clearance_message(1, 1, &addr, &null);
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_next_anchor_not_trivial() {
        let rng = Felt252::from_u64(42);
        let null = Felt252::from_u64(100);
        let cx = Felt252::from_u64(1);
        let cy = Felt252::from_u64(2);
        let anchor = compute_next_anchor(&rng, &null, &cx, &cy, 0);
        assert!(!anchor.is_zero());
    }
}
