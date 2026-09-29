//! Poseidon hash implementation over the Stark field.
//!
//! Uses the Starknet Poseidon hash (hades permutation) compatible with
//! Cairo's builtin Poseidon. All invocations require a domain separation tag.

use starknet_crypto::{poseidon_hash as stark_poseidon, poseidon_hash_many};
use starknet_types_core::felt::Felt;
use zkapi_types::Felt252;

/// Re-export the Felt type so downstream crates can use it without adding
/// starknet-types-core as a direct dependency.
pub type FieldElement = Felt;

/// Convert a Felt252 to starknet FieldElement (Felt).
pub fn felt_to_field(f: &Felt252) -> FieldElement {
    FieldElement::from_bytes_be(f.as_bytes())
}

/// Convert a starknet FieldElement (Felt) to Felt252.
pub fn field_to_felt(f: &FieldElement) -> Felt252 {
    Felt252(f.to_bytes_be())
}

/// Domain-separated Poseidon hash of two inputs.
///
/// H(domain, a, b)
pub fn poseidon_hash(domain: &Felt252, a: &Felt252, b: &Felt252) -> Felt252 {
    let inputs = [felt_to_field(domain), felt_to_field(a), felt_to_field(b)];
    field_to_felt(&poseidon_hash_many(&inputs))
}

/// Domain-separated Poseidon hash of arbitrary number of inputs.
///
/// H(domain, inputs[0], inputs[1], ...)
pub fn poseidon_hash_chain(domain: &Felt252, inputs: &[Felt252]) -> Felt252 {
    let mut all: Vec<FieldElement> = Vec::with_capacity(1 + inputs.len());
    all.push(felt_to_field(domain));
    for inp in inputs {
        all.push(felt_to_field(inp));
    }
    field_to_felt(&poseidon_hash_many(&all))
}

/// Poseidon hash of exactly two field elements (no domain, used for internal nodes when
/// domain is passed as part of the input).
pub fn poseidon_hash_pair(a: &FieldElement, b: &FieldElement) -> FieldElement {
    stark_poseidon(*a, *b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zkapi_types::domain::DOMAIN_REG;

    #[test]
    fn test_poseidon_deterministic() {
        let a = Felt252::from_u64(42);
        let b = Felt252::ZERO;
        let h1 = poseidon_hash(&DOMAIN_REG, &a, &b);
        let h2 = poseidon_hash(&DOMAIN_REG, &a, &b);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_poseidon_different_domains() {
        use zkapi_types::domain::DOMAIN_LEAF;
        let a = Felt252::from_u64(42);
        let b = Felt252::ZERO;
        let h1 = poseidon_hash(&DOMAIN_REG, &a, &b);
        let h2 = poseidon_hash(&DOMAIN_LEAF, &a, &b);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_poseidon_chain() {
        let inputs = [
            Felt252::from_u64(1),
            Felt252::from_u64(2),
            Felt252::from_u64(3),
        ];
        let h = poseidon_hash_chain(&DOMAIN_REG, &inputs);
        let h2 = poseidon_hash_chain(&DOMAIN_REG, &inputs);
        assert_eq!(h, h2);
    }
}
