//! Cryptographic primitives for zkAPI.
//!
//! This crate contains:
//! - Pedersen commitment on the Stark curve (the single PQ exception in v1)
//! - XMSS/WOTS+ hash-based signatures using Poseidon

pub mod pedersen;
pub mod wots;
pub mod xmss;

pub use pedersen::{PedersenCommitment, G_BALANCE, H_BLIND};
pub use xmss::{XmssKeypair, XmssVerifier};
