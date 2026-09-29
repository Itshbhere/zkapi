//! Domain separation tags for the zkAPI protocol.
//!
//! Every Poseidon invocation must use a domain tag. The tag is computed as the
//! big-endian integer of the ASCII bytes of the label, interpreted as a felt.
//! Labels must be at most 31 bytes.

use crate::Felt252;

/// A domain separation tag derived from an ASCII label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainTag {
    pub label: &'static str,
    pub felt: Felt252,
}

/// Compute the domain tag felt from an ASCII label.
///
/// The label bytes are interpreted as a big-endian integer that fits in a felt252.
/// Labels must be at most 31 bytes (to fit in 248 bits with room to spare).
pub const fn domain_felt(label: &[u8]) -> Felt252 {
    assert!(label.len() <= 31, "domain label must be <= 31 bytes");
    let mut bytes = [0u8; 32];
    let offset = 32 - label.len();
    let mut i = 0;
    while i < label.len() {
        bytes[offset + i] = label[i];
        i += 1;
    }
    Felt252(bytes)
}

// All protocol domain tags
pub const DOMAIN_REG: Felt252 = domain_felt(b"zkapi.reg");
pub const DOMAIN_LEAF: Felt252 = domain_felt(b"zkapi.leaf");
pub const DOMAIN_NODE: Felt252 = domain_felt(b"zkapi.node");
pub const DOMAIN_NULL: Felt252 = domain_felt(b"zkapi.null");
pub const DOMAIN_STATE: Felt252 = domain_felt(b"zkapi.state");
pub const DOMAIN_CLEAR: Felt252 = domain_felt(b"zkapi.clear");
pub const DOMAIN_REQPUB: Felt252 = domain_felt(b"zkapi.reqpub");
pub const DOMAIN_WDPUB: Felt252 = domain_felt(b"zkapi.wdpub");
pub const DOMAIN_BAL_G: Felt252 = domain_felt(b"zkapi.bal.g");
pub const DOMAIN_BAL_H: Felt252 = domain_felt(b"zkapi.bal.h");
pub const DOMAIN_XMSS_LEAF: Felt252 = domain_felt(b"zkapi.xmss.leaf");
pub const DOMAIN_XMSS_NODE: Felt252 = domain_felt(b"zkapi.xmss.node");
pub const DOMAIN_XMSS_CHAIN: Felt252 = domain_felt(b"zkapi.xmss.chain");
pub const DOMAIN_XMSS_MSG: Felt252 = domain_felt(b"zkapi.xmss.msg");
pub const DOMAIN_XMSS_SK: Felt252 = domain_felt(b"zkapi.xmss.sk");
pub const DOMAIN_ANCHOR: Felt252 = domain_felt(b"zkapi.anchor");
pub const DOMAIN_BLIND: Felt252 = domain_felt(b"zkapi.blind");

/// All domain tags with their labels, for cross-system validation.
pub const DOMAIN_TAGS: &[DomainTag] = &[
    DomainTag {
        label: "zkapi.reg",
        felt: DOMAIN_REG,
    },
    DomainTag {
        label: "zkapi.leaf",
        felt: DOMAIN_LEAF,
    },
    DomainTag {
        label: "zkapi.node",
        felt: DOMAIN_NODE,
    },
    DomainTag {
        label: "zkapi.null",
        felt: DOMAIN_NULL,
    },
    DomainTag {
        label: "zkapi.state",
        felt: DOMAIN_STATE,
    },
    DomainTag {
        label: "zkapi.clear",
        felt: DOMAIN_CLEAR,
    },
    DomainTag {
        label: "zkapi.reqpub",
        felt: DOMAIN_REQPUB,
    },
    DomainTag {
        label: "zkapi.wdpub",
        felt: DOMAIN_WDPUB,
    },
    DomainTag {
        label: "zkapi.bal.g",
        felt: DOMAIN_BAL_G,
    },
    DomainTag {
        label: "zkapi.bal.h",
        felt: DOMAIN_BAL_H,
    },
    DomainTag {
        label: "zkapi.xmss.leaf",
        felt: DOMAIN_XMSS_LEAF,
    },
    DomainTag {
        label: "zkapi.xmss.node",
        felt: DOMAIN_XMSS_NODE,
    },
    DomainTag {
        label: "zkapi.xmss.chain",
        felt: DOMAIN_XMSS_CHAIN,
    },
    DomainTag {
        label: "zkapi.xmss.msg",
        felt: DOMAIN_XMSS_MSG,
    },
    DomainTag {
        label: "zkapi.xmss.sk",
        felt: DOMAIN_XMSS_SK,
    },
    DomainTag {
        label: "zkapi.anchor",
        felt: DOMAIN_ANCHOR,
    },
    DomainTag {
        label: "zkapi.blind",
        felt: DOMAIN_BLIND,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_tag_encoding() {
        // "zkapi.reg" as ASCII bytes: [122, 107, 97, 112, 105, 46, 114, 101, 103]
        // As big-endian integer
        let tag = domain_felt(b"zkapi.reg");
        assert!(!tag.is_zero());

        // Verify deterministic
        let tag2 = domain_felt(b"zkapi.reg");
        assert_eq!(tag, tag2);

        // Different labels produce different tags
        let tag3 = domain_felt(b"zkapi.leaf");
        assert_ne!(tag, tag3);
    }

    #[test]
    fn test_all_tags_unique() {
        for (i, tag_i) in DOMAIN_TAGS.iter().enumerate() {
            for tag_j in DOMAIN_TAGS.iter().skip(i + 1) {
                assert_ne!(
                    tag_i.felt, tag_j.felt,
                    "duplicate domain tag: {} and {}",
                    tag_i.label, tag_j.label
                );
            }
        }
    }

    #[test]
    fn test_label_length_limit() {
        // 31 bytes should work
        let label = b"1234567890123456789012345678901";
        assert_eq!(label.len(), 31);
        let _ = domain_felt(label);
    }
}
