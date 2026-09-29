// Merkle tree verification for zkAPI.
//
// Uses a fixed-depth binary Merkle tree with depth 32.
// Internal node hash: Poseidon(DOMAIN_NODE, left, right).

use core::poseidon::poseidon_hash_span;
use super::constants::MERKLE_DEPTH;
use super::domains::DOMAIN_NODE;

/// Compute the hash of an internal Merkle tree node.
pub fn node_hash(left: felt252, right: felt252) -> felt252 {
    poseidon_hash_span(array![DOMAIN_NODE, left, right].span())
}

/// Verify that `leaf` is included in the tree with the given `root`.
///
/// `index_bits` contains MERKLE_DEPTH felt252 values, each 0 or 1, representing the
/// path from the leaf to the root (bit 0 = lowest level).
/// `siblings` contains the MERKLE_DEPTH sibling hashes along the path.
pub fn verify_merkle_path(
    root: felt252, leaf: felt252, index_bits: Span<felt252>, siblings: Span<felt252>,
) {
    assert(index_bits.len() == MERKLE_DEPTH, 'invalid index bits len');
    assert(siblings.len() == MERKLE_DEPTH, 'invalid siblings len');

    let mut current = leaf;
    let mut i: u32 = 0;
    while i < MERKLE_DEPTH {
        let bit = *index_bits.at(i);
        assert(bit == 0 || bit == 1, 'invalid index bit');
        let sibling = *siblings.at(i);
        if bit == 0 {
            current = node_hash(current, sibling);
        } else {
            current = node_hash(sibling, current);
        }
        i += 1;
    }
    assert(current == root, 'merkle proof invalid');
}

#[cfg(test)]
mod tests {
    use zkapi_cairo::constants::MERKLE_DEPTH;
    use super::{node_hash, verify_merkle_path};

    fn zero_siblings_and_root(leaf: felt252) -> (Array<felt252>, Array<felt252>, felt252) {
        let mut index_bits = array![];
        let mut siblings = array![];
        let mut zero = 0;
        let mut current = leaf;
        let mut i: u32 = 0;
        while i < MERKLE_DEPTH {
            index_bits.append(0);
            siblings.append(zero);
            current = node_hash(current, zero);
            zero = node_hash(zero, zero);
            i += 1;
        }
        (index_bits, siblings, current)
    }

    #[test]
    fn test_verify_merkle_path_single_leaf() {
        let leaf = 0x1234;
        let (index_bits, siblings, root) = zero_siblings_and_root(leaf);
        verify_merkle_path(root, leaf, index_bits.span(), siblings.span());
    }
}
