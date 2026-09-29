// Domain separation tags for zkAPI v1.
//
// Each constant is the big-endian integer of the ASCII bytes of its label,
// interpreted as a felt252.  All labels are at most 31 bytes.

/// "zkapi.reg"
pub const DOMAIN_REG: felt252 = 0x7a6b6170692e726567;

/// "zkapi.leaf"
pub const DOMAIN_LEAF: felt252 = 0x7a6b6170692e6c656166;

/// "zkapi.node"
pub const DOMAIN_NODE: felt252 = 0x7a6b6170692e6e6f6465;

/// "zkapi.null"
pub const DOMAIN_NULL: felt252 = 0x7a6b6170692e6e756c6c;

/// "zkapi.state"
pub const DOMAIN_STATE: felt252 = 0x7a6b6170692e7374617465;

/// "zkapi.clear"
pub const DOMAIN_CLEAR: felt252 = 0x7a6b6170692e636c656172;

/// "zkapi.reqpub"
pub const DOMAIN_REQPUB: felt252 = 0x7a6b6170692e726571707562;

/// "zkapi.wdpub"
pub const DOMAIN_WDPUB: felt252 = 0x7a6b6170692e7764707562;

/// "zkapi.bal.g"
pub const DOMAIN_BAL_G: felt252 = 0x7a6b6170692e62616c2e67;

/// "zkapi.bal.h"
pub const DOMAIN_BAL_H: felt252 = 0x7a6b6170692e62616c2e68;

/// "zkapi.xmss.leaf"
pub const DOMAIN_XMSS_LEAF: felt252 = 0x7a6b6170692e786d73732e6c656166;

/// "zkapi.xmss.node"
pub const DOMAIN_XMSS_NODE: felt252 = 0x7a6b6170692e786d73732e6e6f6465;

/// "zkapi.xmss.chain"
pub const DOMAIN_XMSS_CHAIN: felt252 = 0x7a6b6170692e786d73732e636861696e;

/// "zkapi.xmss.msg"
pub const DOMAIN_XMSS_MSG: felt252 = 0x7a6b6170692e786d73732e6d7367;

/// "zkapi.anchor"
pub const DOMAIN_ANCHOR: felt252 = 0x7a6b6170692e616e63686f72;

/// "zkapi.blind"
pub const DOMAIN_BLIND: felt252 = 0x7a6b6170692e626c696e64;
