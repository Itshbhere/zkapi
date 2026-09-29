//! Server configuration.

use zkapi_types::{EpochRoots, Felt252};

/// Proof verifier backend used by the request processor.
#[derive(Debug, Clone)]
pub enum ServerProofMode {
    /// Verify real Stwo/Stwo-Cairo proof artifacts through Scarb.
    StwoScarb {
        /// Path to the Cairo package directory.
        cairo_dir: String,
    },
    /// Development-only local witness replay verifier.
    #[cfg(feature = "dev-witness-envelope")]
    DevWitnessEnvelope,
}

/// Configuration for the zkAPI server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Protocol version (must be 1 for v1).
    pub protocol_version: u16,
    /// Chain ID this server is bound to.
    pub chain_id: u64,
    /// On-chain contract address.
    pub contract_address: Felt252,
    /// Maximum charge per request (in base units).
    pub request_charge_cap: u128,
    /// Maximum charge under policy enforcement.
    pub policy_charge_cap: u128,
    /// Whether policy enforcement is enabled.
    pub policy_enabled: bool,
    /// HTTP listen address (e.g. "0.0.0.0:3000").
    pub listen_addr: String,
    /// Path to the SQLite database file.
    pub db_path: String,
    /// Timeout in milliseconds for recovery of reserved-but-unfinalized entries.
    pub recovery_timeout_ms: u64,
    /// Seed for the state-signing XMSS tree.
    pub state_seed: Felt252,
    /// Seed for the clearance-signing XMSS tree.
    pub clear_seed: Felt252,
    /// Published XMSS epoch served by this process.
    pub epoch: u32,
    /// XMSS tree height.
    pub xmss_height: usize,
    /// Initial Merkle root the server should accept until the indexer updates it.
    pub initial_root: Felt252,
    /// Trusted epoch roots this server accepts for prior client states.
    pub trusted_epoch_roots: Vec<EpochRoots>,
    /// Proof backend used for runtime request verification.
    pub proof_mode: ServerProofMode,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            protocol_version: 1,
            chain_id: 1,
            contract_address: Felt252::ZERO,
            request_charge_cap: 1_000_000,
            policy_charge_cap: 10_000_000,
            policy_enabled: false,
            listen_addr: "0.0.0.0:3000".to_string(),
            db_path: "zkapi_server.db".to_string(),
            recovery_timeout_ms: 30_000,
            state_seed: Felt252::from_u64(1),
            clear_seed: Felt252::from_u64(2),
            epoch: 1,
            xmss_height: zkapi_types::XMSS_TREE_HEIGHT,
            initial_root: Felt252::ZERO,
            trusted_epoch_roots: Vec::new(),
            proof_mode: ServerProofMode::StwoScarb {
                cairo_dir: "cairo".to_string(),
            },
        }
    }
}
