// Combined XMSS signature verification.
//
// Ties together WOTS+ verification and the XMSS authentication path.

use core::poseidon::poseidon_hash_span;
use zkapi_cairo::domains::DOMAIN_XMSS_MSG;
use super::tree::verify_auth_path;
use super::wots::{wots_pk_to_leaf, wots_verify};

/// Verify a full XMSS signature.
///
/// Steps:
///   1. Hash the raw `message` with DOMAIN_XMSS_MSG to produce the digest
///      that was actually signed.
///   2. Recover the WOTS+ public key from the signature and digest.
///   3. Hash the recovered public key to get the XMSS leaf.
///   4. Verify the authentication path from the leaf to `root`.
///
/// Panics if any verification step fails.
pub fn verify_xmss(
    root: felt252,
    message: felt252,
    leaf_index: u32,
    wots_sig: Span<felt252>,
    auth_path: Span<felt252>,
) {
    // 1. Domain-separate the message.
    let digest = poseidon_hash_span(array![DOMAIN_XMSS_MSG, message].span());

    // 2. Recover WOTS+ public key values from the signature.
    let pk_values = wots_verify(wots_sig, digest);

    // 3. Hash public key values to get the leaf.
    let leaf = wots_pk_to_leaf(pk_values.span());

    // 4. Verify the authentication path.
    verify_auth_path(leaf, leaf_index, auth_path, root);
}
