//! Note and withdrawal types matching the on-chain data model.

use serde::{Deserialize, Serialize};

use crate::Felt252;

/// On-chain note status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum NoteStatus {
    Uninitialized = 0,
    Active = 1,
    PendingWithdrawal = 2,
    Closed = 3,
}

impl NoteStatus {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Uninitialized),
            1 => Some(Self::Active),
            2 => Some(Self::PendingWithdrawal),
            3 => Some(Self::Closed),
            _ => None,
        }
    }
}

/// On-chain note metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub note_id: u32,
    pub commitment: Felt252,
    pub deposit_amount: u128,
    pub expiry_ts: u64,
    pub status: NoteStatus,
}

/// Pending escape-hatch withdrawal data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWithdrawal {
    pub exists: bool,
    pub withdrawal_nullifier: Felt252,
    pub final_balance: u128,
    pub destination: [u8; 20],
    pub challenge_deadline: u64,
}

/// Nullifier status in the server's transcript store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullifierStatus {
    Reserved,
    Finalized,
    ClearanceReserved,
}
