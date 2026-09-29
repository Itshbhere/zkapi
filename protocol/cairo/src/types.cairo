// Shared types for zkAPI Cairo programs.

/// A point on the Stark curve, used for Pedersen balance commitments.
#[derive(Drop, Copy)]
pub struct CurvePoint {
    pub x: felt252,
    pub y: felt252,
}

/// XMSS signature components.
/// In practice these are passed as separate Span arguments to avoid
/// fixed-size array limitations in Cairo.
#[derive(Drop, Copy)]
pub struct XmssSignatureParams {
    pub epoch: u32,
    pub leaf_index: u32,
}
