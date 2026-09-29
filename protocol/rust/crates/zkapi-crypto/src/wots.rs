//! WOTS+ one-time signature scheme over Poseidon.
//!
//! Parameters (from spec):
//! - w = 16 (Winternitz parameter)
//! - n = 248 bits (digest length, fits in one felt)
//! - len1 = ceil(248/4) = 62
//! - len2 = 3
//! - len = 65
//!
//! Chain function: F(domain("zkapi.xmss.chain"), x, i) where i is the chain step.

use starknet_crypto::poseidon_hash_many;
use starknet_types_core::felt::Felt;
use zkapi_types::{WOTS_LEN, WOTS_LEN1, WOTS_LEN2, WOTS_W};

use zkapi_core::poseidon::felt_to_field;

/// Type alias for downstream compatibility.
type FieldElement = Felt;

/// WOTS+ chain step: applies the chain function once.
///
/// F(domain("zkapi.xmss.chain"), value, step_index)
fn chain_step(value: &FieldElement, step: u32) -> FieldElement {
    let domain = felt_to_field(&zkapi_types::domain::DOMAIN_XMSS_CHAIN);
    poseidon_hash_many(&[domain, *value, FieldElement::from(step as u64)])
}

/// Apply the chain function `steps` times starting from `value`.
fn chain(value: &FieldElement, start: u32, steps: u32) -> FieldElement {
    let mut current = *value;
    for i in start..start + steps {
        current = chain_step(&current, i);
    }
    current
}

/// Compute the WOTS+ public key from a secret key (array of len random felts).
pub fn wots_keygen(secret_key: &[FieldElement; WOTS_LEN]) -> [FieldElement; WOTS_LEN] {
    let mut public_key = [FieldElement::ZERO; WOTS_LEN];
    for i in 0..WOTS_LEN {
        // Chain from 0 to w-1 steps
        public_key[i] = chain(&secret_key[i], 0, (WOTS_W - 1) as u32);
    }
    public_key
}

/// Compute the WOTS+ leaf hash from a public key.
///
/// leaf = Poseidon(domain("zkapi.xmss.leaf"), pk[0], pk[1], ..., pk[64])
pub fn wots_pk_to_leaf(public_key: &[FieldElement; WOTS_LEN]) -> FieldElement {
    let domain = felt_to_field(&zkapi_types::domain::DOMAIN_XMSS_LEAF);
    let mut inputs = Vec::with_capacity(1 + WOTS_LEN);
    inputs.push(domain);
    inputs.extend_from_slice(public_key);
    poseidon_hash_many(&inputs)
}

/// Convert a message digest (248 bits from a felt) into base-w digits.
///
/// Returns (msg_digits[len1], checksum_digits[len2]).
pub fn message_to_base_w(message: &FieldElement) -> ([u32; WOTS_LEN1], [u32; WOTS_LEN2]) {
    let bytes = message.to_bytes_be();

    // Extract 248 bits = 62 nibbles (4 bits each for w=16)
    // We use bits [8..256] of the 256-bit representation (skip top 8 bits to get 248 bits)
    let mut msg_digits = [0u32; WOTS_LEN1];
    for (i, digit) in msg_digits.iter_mut().enumerate().take(WOTS_LEN1) {
        // Each digit is 4 bits
        let bit_offset = 8 + i * 4; // start from bit 8
        let byte_idx = bit_offset / 8;
        let bit_in_byte = bit_offset % 8;

        if bit_in_byte <= 4 {
            *digit = ((bytes[byte_idx] >> (4 - bit_in_byte)) & 0x0F) as u32;
        } else {
            // Spans two bytes
            let high = (bytes[byte_idx] << (bit_in_byte - 4)) & 0x0F;
            let low = bytes[byte_idx + 1] >> (12 - bit_in_byte);
            *digit = (high | low) as u32;
        }
    }

    // Compute checksum
    let mut checksum: u32 = 0;
    for &d in &msg_digits {
        checksum += (WOTS_W as u32 - 1) - d;
    }

    // Encode checksum in base-w with len2 digits
    let mut cs_digits = [0u32; WOTS_LEN2];
    let mut cs = checksum;
    for i in (0..WOTS_LEN2).rev() {
        cs_digits[i] = cs % WOTS_W as u32;
        cs /= WOTS_W as u32;
    }

    (msg_digits, cs_digits)
}

/// Sign a message digest with a WOTS+ secret key.
pub fn wots_sign(
    secret_key: &[FieldElement; WOTS_LEN],
    message: &FieldElement,
) -> [FieldElement; WOTS_LEN] {
    let (msg_digits, cs_digits) = message_to_base_w(message);
    let mut signature = [FieldElement::ZERO; WOTS_LEN];

    for i in 0..WOTS_LEN1 {
        signature[i] = chain(&secret_key[i], 0, msg_digits[i]);
    }
    for i in 0..WOTS_LEN2 {
        signature[WOTS_LEN1 + i] = chain(&secret_key[WOTS_LEN1 + i], 0, cs_digits[i]);
    }

    signature
}

/// Verify a WOTS+ signature by recovering the public key.
///
/// Returns the recovered public key values (which should be hashed to a leaf).
pub fn wots_verify(
    signature: &[FieldElement; WOTS_LEN],
    message: &FieldElement,
) -> [FieldElement; WOTS_LEN] {
    let (msg_digits, cs_digits) = message_to_base_w(message);
    let mut recovered_pk = [FieldElement::ZERO; WOTS_LEN];

    for i in 0..WOTS_LEN1 {
        let remaining = (WOTS_W as u32 - 1) - msg_digits[i];
        recovered_pk[i] = chain(&signature[i], msg_digits[i], remaining);
    }
    for i in 0..WOTS_LEN2 {
        let remaining = (WOTS_W as u32 - 1) - cs_digits[i];
        recovered_pk[WOTS_LEN1 + i] = chain(&signature[WOTS_LEN1 + i], cs_digits[i], remaining);
    }

    recovered_pk
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_secret_key() -> [FieldElement; WOTS_LEN] {
        let mut sk = [FieldElement::ZERO; WOTS_LEN];
        for (i, item) in sk.iter_mut().enumerate().take(WOTS_LEN) {
            *item = FieldElement::from(i as u64 + 1);
        }
        sk
    }

    #[test]
    fn test_chain_step_deterministic() {
        let val = FieldElement::from(42u64);
        let r1 = chain_step(&val, 0);
        let r2 = chain_step(&val, 0);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_wots_sign_verify() {
        let sk = random_secret_key();
        let pk = wots_keygen(&sk);
        let message = FieldElement::from(12345u64);

        let sig = wots_sign(&sk, &message);
        let recovered = wots_verify(&sig, &message);

        // Recovered pk should match original
        for i in 0..WOTS_LEN {
            assert_eq!(pk[i], recovered[i], "mismatch at index {}", i);
        }
    }

    #[test]
    fn test_wots_wrong_message() {
        let sk = random_secret_key();
        let pk = wots_keygen(&sk);
        let message = FieldElement::from(12345u64);
        let wrong_message = FieldElement::from(54321u64);

        let sig = wots_sign(&sk, &message);
        let recovered = wots_verify(&sig, &wrong_message);

        // Should NOT match
        let mut matches = true;
        for i in 0..WOTS_LEN {
            if pk[i] != recovered[i] {
                matches = false;
                break;
            }
        }
        assert!(!matches);
    }

    #[test]
    fn test_checksum_bounds() {
        // All zeros message
        let msg = FieldElement::ZERO;
        let (msg_digits, cs_digits) = message_to_base_w(&msg);

        // With all zero digits, checksum = len1 * (w-1) = 62 * 15 = 930
        let checksum: u32 = msg_digits.iter().map(|&d| 15 - d).sum();
        assert!(checksum <= 930);

        // Checksum should fit in len2=3 base-16 digits (max 16^3 - 1 = 4095)
        let mut cs_val = 0u32;
        for &d in &cs_digits {
            cs_val = cs_val * 16 + d;
        }
        assert_eq!(cs_val, checksum);
    }
}
