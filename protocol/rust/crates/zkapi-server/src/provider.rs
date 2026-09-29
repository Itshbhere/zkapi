//! API execution boundary for the zkAPI server.
//!
//! The zk layer and billing logic are protocol code. The actual upstream API
//! execution is application-specific, so the server uses a provider trait with
//! a small deterministic implementation for local development/tests.

use crate::error::ServerError;
use zkapi_types::Felt252;

/// Result of executing the upstream API call.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub status_code: u16,
    pub payload: String,
    pub charge_applied: u128,
    pub policy_reason_code: Option<u32>,
    pub policy_evidence_hash: Option<Felt252>,
}

/// Application-specific API executor.
pub trait ApiProvider: Send + Sync {
    fn execute(
        &self,
        client_request_id: &str,
        payload: &str,
        payload_hash: &Felt252,
    ) -> Result<ProviderResponse, ServerError>;
}

/// Deterministic local provider used by tests and the default CLI server.
///
/// It echoes the payload back to the client and charges a fixed amount. The
/// `client_request_id` parameter exists to support idempotent implementations
/// in real deployments.
pub struct EchoProvider {
    fixed_charge: u128,
}

impl EchoProvider {
    pub fn new(fixed_charge: u128) -> Self {
        Self { fixed_charge }
    }
}

impl Default for EchoProvider {
    fn default() -> Self {
        Self { fixed_charge: 1 }
    }
}

impl ApiProvider for EchoProvider {
    fn execute(
        &self,
        _client_request_id: &str,
        payload: &str,
        _payload_hash: &Felt252,
    ) -> Result<ProviderResponse, ServerError> {
        Ok(ProviderResponse {
            status_code: 200,
            payload: payload.to_string(),
            charge_applied: self.fixed_charge,
            policy_reason_code: None,
            policy_evidence_hash: None,
        })
    }
}
