//! Main request processing logic.
//!
//! Follows the server flow from spec section 9.3:
//! 1. Validate public inputs against config
//! 2. Verify the opaque proof artifact
//! 3. Reserve nullifier
//! 4. Execute provider call
//! 5. Compute charge, anchor, blind delta, next commitment
//! 6. Sign the next state
//! 7. Finalize transcript
//! 8. Return response

use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use zkapi_core::commitment::{compute_blind_delta, compute_next_anchor, compute_state_message};
use zkapi_core::poseidon::{felt_to_field, field_to_felt};
use zkapi_crypto::pedersen::PedersenCommitment;
#[cfg(feature = "dev-witness-envelope")]
use zkapi_proof::verify_request_proof;
use zkapi_proof::{ProofArtifact, ScarbStwoProver};
use zkapi_types::wire::{
    ApiRequest, ClearanceRequest, ClearanceResponse, CurvePointWire, ProofBackendWire,
    RecoveryResponse, RequestResponse,
};
use zkapi_types::{
    canonical_payload_hash, canonical_response_hash, lookup_state_root, Felt252, NullifierStatus,
    STATEMENT_TYPE_REQUEST,
};

use crate::config::{ServerConfig, ServerProofMode};
use crate::error::ServerError;
use crate::nullifier_store::{NullifierStore, TranscriptRecord};
use crate::provider::ApiProvider;
use crate::signer::ServerSigner;

/// The main request processor for the zkAPI server.
pub struct RequestProcessor {
    config: ServerConfig,
    store: Arc<NullifierStore>,
    signer: Arc<ServerSigner>,
    provider: Arc<dyn ApiProvider>,
    current_root: Arc<RwLock<Felt252>>,
}

impl RequestProcessor {
    /// Create a new request processor.
    pub fn new(
        config: ServerConfig,
        store: Arc<NullifierStore>,
        signer: Arc<ServerSigner>,
        provider: Arc<dyn ApiProvider>,
        current_root: Felt252,
    ) -> Self {
        Self {
            config,
            store,
            signer,
            provider,
            current_root: Arc::new(RwLock::new(current_root)),
        }
    }

    /// Update the current Merkle root (called when the indexer detects a new root).
    pub fn update_root(&self, new_root: Felt252) {
        if let Ok(mut root) = self.current_root.write() {
            *root = new_root;
        }
    }

    /// Get the current Merkle root.
    pub fn current_root(&self) -> Felt252 {
        self.current_root
            .read()
            .map(|r| *r)
            .unwrap_or(Felt252::ZERO)
    }

    /// Process an API request following spec section 9.3.
    ///
    /// Returns either a successful RequestResponse or a ServerError.
    pub fn process_request(
        &self,
        api_request: &ApiRequest,
    ) -> Result<RequestResponse, ServerError> {
        let pi = &api_request.public_inputs;
        let actual_payload_hash = canonical_payload_hash(api_request.payload.as_bytes());
        if api_request.payload_hash != actual_payload_hash {
            return Err(ServerError::InvalidRequest(
                "payload_hash does not match actual payload bytes".to_string(),
            ));
        }

        // Step 1: Validate protocol version
        if pi.protocol_version != self.config.protocol_version {
            return Err(ServerError::ProtocolMismatch(format!(
                "expected protocol_version={}, got {}",
                self.config.protocol_version, pi.protocol_version
            )));
        }

        // Step 2: Validate chain_id
        if pi.chain_id != self.config.chain_id {
            return Err(ServerError::ProtocolMismatch(format!(
                "expected chain_id={}, got {}",
                self.config.chain_id, pi.chain_id
            )));
        }

        // Step 3: Validate contract_address
        if pi.contract_address != self.config.contract_address {
            return Err(ServerError::ProtocolMismatch(format!(
                "contract_address mismatch: expected {}, got {}",
                self.config.contract_address, pi.contract_address
            )));
        }

        // Step 4: Check active_root matches current root
        let root = self.current_root();
        if pi.active_root != root {
            return Err(ServerError::StaleRoot {
                latest_root: root.to_hex(),
            });
        }

        // Step 5: Check expiry_ts > now
        let now = current_timestamp();
        if pi.expiry_ts <= now {
            return Err(ServerError::NoteExpired);
        }

        // Step 6: Check solvency_bound matches the active server policy.
        let required_solvency_bound = if self.config.policy_enabled {
            self.config.policy_charge_cap
        } else {
            self.config.request_charge_cap
        };
        if pi.solvency_bound < required_solvency_bound {
            return Err(ServerError::InvalidRequest(format!(
                "solvency_bound {} is less than request_charge_cap {}",
                pi.solvency_bound, required_solvency_bound
            )));
        }

        // Step 7: Validate statement_type
        if pi.statement_type != STATEMENT_TYPE_REQUEST {
            return Err(ServerError::InvalidRequest(format!(
                "expected statement_type={}, got {}",
                STATEMENT_TYPE_REQUEST, pi.statement_type
            )));
        }

        // Step 8: Verify state_sig_epoch/root consistency. Non-genesis
        // requests may reference any trusted published epoch, not only this
        // process's current signing tree.
        validate_state_sig_root(
            &self.config,
            &self.signer,
            pi.state_sig_epoch,
            pi.state_sig_root,
        )?;

        // Step 9: Verify the proof artifact against the stated public inputs.
        let proof_bytes = self.verify_request_proof_artifact(api_request)?;

        // Step 10: Reserve nullifier in store
        match self.store.lookup_by_nullifier(&pi.request_nullifier) {
            Some(existing)
                if existing.client_request_id.as_deref()
                    == Some(&api_request.client_request_id)
                    && existing.payload_hash == Some(api_request.payload_hash)
                    && existing.status == NullifierStatus::Finalized =>
            {
                return build_response_from_record(&existing, &api_request.client_request_id);
            }
            Some(existing)
                if existing.client_request_id.as_deref()
                    == Some(&api_request.client_request_id)
                    && existing.payload_hash == Some(api_request.payload_hash)
                    && existing.status == NullifierStatus::Reserved => {}
            Some(_) => return Err(ServerError::Replay),
            None => self.store.reserve(
                &pi.request_nullifier,
                &api_request.client_request_id,
                &api_request.payload_hash,
            )?,
        }

        // Step 11: Execute the upstream provider call.
        let provider_response = self.provider.execute(
            &api_request.client_request_id,
            &api_request.payload,
            &api_request.payload_hash,
        )?;
        let response_code = provider_response.status_code;
        let response_payload = provider_response.payload;
        let response_hash = canonical_response_hash(response_payload.as_bytes());
        let charge = provider_response.charge_applied;

        // Step 12: Enforce the charge cap before signing a next state.
        let max_charge = if self.config.policy_enabled {
            self.config.policy_charge_cap
        } else {
            self.config.request_charge_cap
        };
        if charge > max_charge {
            return Err(ServerError::Internal(format!(
                "provider charge {} exceeds cap {}",
                charge, max_charge
            )));
        }

        let server_rng = generate_server_rng(&pi.request_nullifier);
        let server_rng2 = generate_server_rng2(&pi.request_nullifier);

        // Step 14: Compute the next commitment homomorphically.
        //
        // next_commitment = anon_commitment - charge * G_balance + blind_delta * H_blind
        //
        // The anon_commitment comes from the proof's public inputs (anon_commitment_x, anon_commitment_y).
        let anon_point = reconstruct_affine_point(&pi.anon_commitment_x, &pi.anon_commitment_y);

        let mut signed_state: Option<(Felt252, Felt252, Felt252, Felt252)> = None;
        let (state_sig, _leaf_index, _state_msg) = self.signer.sign_next_state(|leaf_index| {
            let blind_delta_felt =
                compute_blind_delta(&server_rng2, &pi.request_nullifier, leaf_index);
            let blind_delta_field = felt_to_field(&blind_delta_felt);
            let updated =
                PedersenCommitment::server_update(&anon_point, charge, &blind_delta_field);
            let (next_cx_field, next_cy_field) = updated.to_affine();
            let next_cx = field_to_felt(&next_cx_field);
            let next_cy = field_to_felt(&next_cy_field);
            let next_anchor = compute_next_anchor(
                &server_rng,
                &pi.request_nullifier,
                &next_cx,
                &next_cy,
                leaf_index,
            );
            let state_msg = compute_state_message(
                self.config.protocol_version,
                self.config.chain_id,
                &self.config.contract_address,
                &next_cx,
                &next_cy,
                &next_anchor,
            );
            signed_state = Some((next_cx, next_cy, next_anchor, blind_delta_felt));
            state_msg
        })?;
        let (next_cx, next_cy, next_anchor, blind_delta_felt) = signed_state
            .ok_or_else(|| ServerError::Internal("signer did not build state".to_string()))?;

        // Step 17: Finalize transcript
        let transcript = TranscriptRecord {
            nullifier: pi.request_nullifier,
            status: NullifierStatus::Finalized,
            client_request_id: Some(api_request.client_request_id.clone()),
            payload_hash: Some(api_request.payload_hash),
            charge_applied: Some(charge),
            response_code: Some(response_code),
            response_payload: Some(response_payload.clone()),
            response_hash: Some(response_hash),
            next_commitment_x: Some(next_cx),
            next_commitment_y: Some(next_cy),
            next_anchor: Some(next_anchor),
            blind_delta_srv: Some(blind_delta_felt),
            next_state_sig_epoch: Some(state_sig.epoch),
            next_state_sig_root: Some(self.signer.state_root()),
            next_state_sig: Some(state_sig.clone()),
            policy_reason_code: provider_response.policy_reason_code,
            policy_evidence_hash: provider_response.policy_evidence_hash,
            proof_blob: Some(proof_bytes.clone()),
            request_inputs_json: serde_json::to_string(&pi).ok(),
            created_at: current_timestamp(),
            finalized_at: Some(current_timestamp()),
        };

        self.store
            .finalize(&pi.request_nullifier, &transcript)
            .map_err(|e| ServerError::Internal(format!("failed to finalize transcript: {}", e)))?;

        // Step 18: Return response
        Ok(RequestResponse {
            status: "ok".to_string(),
            client_request_id: api_request.client_request_id.clone(),
            request_nullifier: pi.request_nullifier,
            response_code,
            response_payload,
            response_hash,
            charge_applied: charge,
            next_commitment: CurvePointWire {
                x: next_cx,
                y: next_cy,
            },
            next_anchor,
            blind_delta_srv: blind_delta_felt,
            next_state_sig_epoch: state_sig.epoch,
            next_state_sig_root: self.signer.state_root(),
            next_state_sig: state_sig,
            policy_reason_code: provider_response.policy_reason_code,
            policy_evidence_hash: provider_response.policy_evidence_hash,
        })
    }

    fn verify_request_proof_artifact(
        &self,
        api_request: &ApiRequest,
    ) -> Result<Vec<u8>, ServerError> {
        let pi = &api_request.public_inputs;
        let artifact = &api_request.proof;
        if artifact.backend != ProofBackendWire::StwoCairo {
            return Err(ServerError::InvalidProof(
                "unsupported proof backend".to_string(),
            ));
        }
        let expected_hash = pi.public_output_hash();
        if artifact.public_output_hash != expected_hash {
            return Err(ServerError::InvalidProof(
                "proof public_output_hash does not match request public inputs".to_string(),
            ));
        }
        let proof_bytes = base64::engine::general_purpose::STANDARD
            .decode(artifact.proof.as_bytes())
            .map_err(|e| ServerError::InvalidProof(format!("invalid base64 proof: {}", e)))?;

        match &self.config.proof_mode {
            ServerProofMode::StwoScarb { cairo_dir } => {
                let proof_artifact =
                    ProofArtifact::stwo_cairo(artifact.public_output_hash, proof_bytes.clone());
                ScarbStwoProver::new(cairo_dir)
                    .verify_artifact(&proof_artifact)
                    .map_err(|e| ServerError::InvalidProof(e.to_string()))?;
            }
            #[cfg(feature = "dev-witness-envelope")]
            ServerProofMode::DevWitnessEnvelope => {
                verify_request_proof(&proof_bytes, pi)
                    .map_err(|e| ServerError::InvalidProof(e.to_string()))?;
            }
        }

        Ok(proof_bytes)
    }

    /// Process a clearance request for mutual close.
    ///
    /// 1. Check nullifier not already used
    /// 2. Sign clearance message
    /// 3. Reserve clearance nullifier
    pub fn process_clearance(
        &self,
        clearance_req: &ClearanceRequest,
    ) -> Result<ClearanceResponse, ServerError> {
        let nullifier = &clearance_req.withdrawal_nullifier;

        // Step 1: Check nullifier not already used
        if self.store.lookup_by_nullifier(nullifier).is_some() {
            return Err(ServerError::NullifierUsed);
        }

        // Step 2: Compute clearance message and sign
        let clear_msg = zkapi_core::commitment::compute_clearance_message(
            self.config.protocol_version,
            self.config.chain_id,
            &self.config.contract_address,
            nullifier,
        );

        let (clear_sig, _leaf_index) = self.signer.sign_clearance(&clear_msg)?;

        // Step 3: Reserve clearance nullifier
        self.store.reserve_clearance(nullifier)?;

        Ok(ClearanceResponse {
            status: "ok".to_string(),
            withdrawal_nullifier: *nullifier,
            clear_sig_epoch: clear_sig.epoch,
            clear_sig_root: self.signer.clear_root(),
            clear_sig,
        })
    }

    /// Recovery: look up a transcript by client request ID.
    pub fn recover_by_client_id(
        &self,
        client_request_id: &str,
    ) -> Result<RecoveryResponse, ServerError> {
        match self.store.lookup_by_client_id(client_request_id) {
            Some(record) => Ok(build_recovery_response(&record)),
            None => Ok(RecoveryResponse {
                status: "not_found".to_string(),
                nullifier_status: "unknown".to_string(),
                request_response: None,
            }),
        }
    }

    /// Recovery: look up a transcript by nullifier.
    pub fn recover_by_nullifier(
        &self,
        nullifier: &Felt252,
    ) -> Result<RecoveryResponse, ServerError> {
        match self.store.lookup_by_nullifier(nullifier) {
            Some(record) => Ok(build_recovery_response(&record)),
            None => Ok(RecoveryResponse {
                status: "not_found".to_string(),
                nullifier_status: "unknown".to_string(),
                request_response: None,
            }),
        }
    }

    /// Get a reference to the config.
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Get a reference to the store.
    pub fn store(&self) -> &Arc<NullifierStore> {
        &self.store
    }
}

/// Build a RequestResponse from a stored TranscriptRecord (for idempotent retries).
fn build_response_from_record(
    record: &TranscriptRecord,
    client_request_id: &str,
) -> Result<RequestResponse, ServerError> {
    Ok(RequestResponse {
        status: "ok".to_string(),
        client_request_id: client_request_id.to_string(),
        request_nullifier: record.nullifier,
        response_code: record.response_code.unwrap_or(200),
        response_payload: record.response_payload.clone().unwrap_or_default(),
        response_hash: record.response_hash.unwrap_or(Felt252::ZERO),
        charge_applied: record.charge_applied.unwrap_or(0),
        next_commitment: CurvePointWire {
            x: record.next_commitment_x.unwrap_or(Felt252::ZERO),
            y: record.next_commitment_y.unwrap_or(Felt252::ZERO),
        },
        next_anchor: record.next_anchor.unwrap_or(Felt252::ZERO),
        blind_delta_srv: record.blind_delta_srv.unwrap_or(Felt252::ZERO),
        next_state_sig_epoch: record.next_state_sig_epoch.unwrap_or(0),
        next_state_sig_root: record.next_state_sig_root.unwrap_or(Felt252::ZERO),
        next_state_sig: record.next_state_sig.clone().unwrap_or_else(empty_xmss_sig),
        policy_reason_code: record.policy_reason_code,
        policy_evidence_hash: record.policy_evidence_hash,
    })
}

/// Build a RecoveryResponse from a TranscriptRecord.
fn build_recovery_response(record: &TranscriptRecord) -> RecoveryResponse {
    let nullifier_status = match record.status {
        NullifierStatus::Reserved => "reserved".to_string(),
        NullifierStatus::Finalized => "finalized".to_string(),
        NullifierStatus::ClearanceReserved => "clearance_reserved".to_string(),
    };

    let request_response = if record.status == NullifierStatus::Finalized {
        let client_id = record.client_request_id.clone().unwrap_or_default();
        build_response_from_record(record, &client_id).ok()
    } else {
        None
    };

    RecoveryResponse {
        status: "ok".to_string(),
        nullifier_status,
        request_response,
    }
}

/// Reconstruct an affine point from x,y Felt252 coordinates into a ProjectivePoint.
fn reconstruct_affine_point(
    x: &Felt252,
    y: &Felt252,
) -> starknet_types_core::curve::ProjectivePoint {
    let x_field = felt_to_field(x);
    let y_field = felt_to_field(y);
    starknet_types_core::curve::ProjectivePoint::from_affine(x_field, y_field)
        .expect("invalid affine point")
}

fn validate_state_sig_root(
    config: &ServerConfig,
    signer: &ServerSigner,
    epoch: u32,
    root: Felt252,
) -> Result<(), ServerError> {
    if epoch == 0 {
        if !root.is_zero() {
            return Err(ServerError::InvalidRequest(
                "genesis state_sig_root must be zero".to_string(),
            ));
        }
        return Ok(());
    }

    if root == signer.state_root() {
        return Ok(());
    }

    let trusted = lookup_state_root(&config.trusted_epoch_roots, epoch);
    if trusted == Some(root) {
        return Ok(());
    }

    Err(ServerError::InvalidRequest(
        "state_sig_root is not trusted for state_sig_epoch".to_string(),
    ))
}

/// Generate a deterministic server RNG value from the nullifier (for anchor derivation).
///
/// In production this would use a proper server-side secret + CSPRNG.
/// For the mock implementation we derive it deterministically from the nullifier.
fn generate_server_rng(nullifier: &Felt252) -> Felt252 {
    use zkapi_types::domain::DOMAIN_ANCHOR;
    zkapi_core::poseidon::poseidon_hash(&DOMAIN_ANCHOR, nullifier, &Felt252::from_u64(1))
}

/// Generate a second deterministic server RNG value (for blind delta derivation).
fn generate_server_rng2(nullifier: &Felt252) -> Felt252 {
    use zkapi_types::domain::DOMAIN_BLIND;
    zkapi_core::poseidon::poseidon_hash(&DOMAIN_BLIND, nullifier, &Felt252::from_u64(2))
}

/// Create an empty XMSS signature placeholder.
fn empty_xmss_sig() -> zkapi_types::XmssSignature {
    zkapi_types::XmssSignature {
        epoch: 0,
        leaf_index: 0,
        wots_sig: vec![Felt252::ZERO; zkapi_types::WOTS_LEN],
        auth_path: vec![Felt252::ZERO; zkapi_types::XMSS_TREE_HEIGHT],
    }
}

/// Get the current UNIX timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use zkapi_core::leaf::{compute_note_leaf, compute_registration_commitment};
    use zkapi_core::merkle::MerkleTree;
    use zkapi_core::poseidon::FieldElement;
    use zkapi_proof::{RequestProofBuilder, ScarbStwoProver};
    use zkapi_types::wire::{ProofArtifactWire, ProofBackendWire};
    use zkapi_types::EpochRoots;

    fn processor_config(trusted_epoch_roots: Vec<EpochRoots>) -> ServerConfig {
        ServerConfig {
            protocol_version: 1,
            chain_id: 1,
            contract_address: Felt252::from_u64(0xdead),
            request_charge_cap: 100,
            policy_charge_cap: 100,
            policy_enabled: false,
            trusted_epoch_roots,
            ..ServerConfig::default()
        }
    }

    fn signer(epoch: u32) -> Arc<ServerSigner> {
        Arc::new(ServerSigner::with_height(
            FieldElement::from(777u64),
            FieldElement::from(888u64),
            epoch,
            4,
        ))
    }

    fn request_processor(config: ServerConfig) -> RequestProcessor {
        RequestProcessor::new(
            config,
            Arc::new(NullifierStore::in_memory().unwrap()),
            signer(1),
            Arc::new(crate::provider::EchoProvider::default()),
            Felt252::from_u64(11),
        )
    }

    fn cairo_dir() -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../cairo")
            .canonicalize()
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../cairo")
            })
            .to_string_lossy()
            .to_string()
    }

    #[test]
    fn state_sig_root_validation_accepts_trusted_prior_epoch_root() {
        let prior_root = Felt252::from_u64(0xabc);
        let signer = signer(2);
        let config = processor_config(vec![EpochRoots {
            epoch: 1,
            state_root: prior_root,
            clear_root: Felt252::from_u64(0xc1ea),
        }]);

        validate_state_sig_root(&config, &signer, 1, prior_root).unwrap();
    }

    #[test]
    fn state_sig_root_validation_rejects_unknown_prior_epoch_root() {
        let config = processor_config(Vec::new());
        let signer = signer(2);

        let err =
            validate_state_sig_root(&config, &signer, 1, Felt252::from_u64(0xabc)).unwrap_err();
        assert!(matches!(err, ServerError::InvalidRequest(msg) if msg.contains("not trusted")));
    }

    #[test]
    fn state_sig_root_validation_rejects_nonzero_genesis_root() {
        let config = processor_config(Vec::new());
        let signer = signer(2);

        let err =
            validate_state_sig_root(&config, &signer, 0, Felt252::from_u64(0xabc)).unwrap_err();
        assert!(matches!(err, ServerError::InvalidRequest(msg) if msg.contains("genesis")));
    }

    #[test]
    fn proof_artifact_rejects_public_output_hash_mismatch() {
        let processor = request_processor(processor_config(Vec::new()));
        let public_inputs = zkapi_types::RequestPublicInputs {
            statement_type: zkapi_types::STATEMENT_TYPE_REQUEST,
            protocol_version: 1,
            chain_id: 1,
            contract_address: Felt252::from_u64(0xdead),
            active_root: Felt252::from_u64(11),
            state_sig_epoch: 0,
            state_sig_root: Felt252::ZERO,
            request_nullifier: Felt252::from_u64(99),
            anon_commitment_x: Felt252::from_u64(1),
            anon_commitment_y: Felt252::from_u64(2),
            expiry_ts: 2_000_000_000,
            solvency_bound: 100,
        };
        let request = ApiRequest {
            client_request_id: "req-1".to_string(),
            payload: "payload".to_string(),
            payload_hash: zkapi_types::canonical_payload_hash(b"payload"),
            public_inputs,
            proof: ProofArtifactWire {
                backend: ProofBackendWire::StwoCairo,
                public_output_hash: Felt252::from_u64(0xbad),
                proof: base64::engine::general_purpose::STANDARD.encode(b"not a proof"),
            },
        };

        let err = processor
            .verify_request_proof_artifact(&request)
            .unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidProof(msg) if msg.contains("public_output_hash"))
        );
    }

    #[test]
    fn proof_artifact_rejects_tampered_stwo_proof_bytes() {
        let mut config = processor_config(Vec::new());
        config.proof_mode = ServerProofMode::StwoScarb {
            cairo_dir: cairo_dir(),
        };
        let processor = request_processor(config);
        let public_inputs = zkapi_types::RequestPublicInputs {
            statement_type: zkapi_types::STATEMENT_TYPE_REQUEST,
            protocol_version: 1,
            chain_id: 1,
            contract_address: Felt252::from_u64(0xdead),
            active_root: Felt252::from_u64(11),
            state_sig_epoch: 0,
            state_sig_root: Felt252::ZERO,
            request_nullifier: Felt252::from_u64(99),
            anon_commitment_x: Felt252::from_u64(1),
            anon_commitment_y: Felt252::from_u64(2),
            expiry_ts: 2_000_000_000,
            solvency_bound: 100,
        };
        let request = ApiRequest {
            client_request_id: "req-1".to_string(),
            payload: "payload".to_string(),
            payload_hash: zkapi_types::canonical_payload_hash(b"payload"),
            proof: ProofArtifactWire {
                backend: ProofBackendWire::StwoCairo,
                public_output_hash: public_inputs.public_output_hash(),
                proof: base64::engine::general_purpose::STANDARD.encode(b"not a proof"),
            },
            public_inputs,
        };

        let err = processor
            .verify_request_proof_artifact(&request)
            .unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidProof(msg) if msg.contains("scarb command failed"))
        );
    }

    #[test]
    fn process_request_accepts_real_stwo_request_artifact() {
        let secret = Felt252::from_u64(42);
        let note_id = 0;
        let deposit = 1_000u128;
        let expiry = 4_000_000_000u64;
        let commitment = compute_registration_commitment(&secret);
        let leaf = compute_note_leaf(note_id, &commitment, deposit, expiry);
        let mut tree = MerkleTree::new();
        tree.insert(leaf);
        let active_root = tree.root();
        let siblings = tree.get_siblings(note_id);

        let mut config = processor_config(Vec::new());
        config.initial_root = active_root;
        config.proof_mode = ServerProofMode::StwoScarb {
            cairo_dir: cairo_dir(),
        };
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        let processor = RequestProcessor::new(
            config.clone(),
            store.clone(),
            signer(1),
            Arc::new(crate::provider::EchoProvider::default()),
            active_root,
        );

        let builder = RequestProofBuilder::new(
            secret,
            note_id,
            deposit,
            expiry,
            siblings,
            deposit,
            FieldElement::ZERO,
            FieldElement::from(7u64),
            Felt252::ONE,
            true,
            0,
            Felt252::ZERO,
            active_root,
            config.protocol_version,
            config.chain_id,
            config.contract_address,
            config.request_charge_cap,
        );
        let public_inputs = builder.build_public_inputs().unwrap();
        let artifact = ScarbStwoProver::new(cairo_dir())
            .prove_and_verify_executable_with_args(
                "request_from_args",
                public_inputs.public_output_hash(),
                &builder.to_cairo_args(None).unwrap(),
            )
            .unwrap();
        let request = ApiRequest {
            client_request_id: "req-real-proof".to_string(),
            payload: "payload".to_string(),
            payload_hash: zkapi_types::canonical_payload_hash(b"payload"),
            public_inputs,
            proof: ProofArtifactWire {
                backend: ProofBackendWire::StwoCairo,
                public_output_hash: artifact.public_output_hash,
                proof: base64::engine::general_purpose::STANDARD.encode(artifact.proof),
            },
        };

        let response = processor.process_request(&request).unwrap();
        assert_eq!(response.status, "ok");
        assert_eq!(response.response_payload, "payload");
        assert_eq!(response.charge_applied, 1);

        let recovery = processor
            .recover_by_client_id("req-real-proof")
            .expect("recovery should read finalized transcript");
        assert_eq!(recovery.nullifier_status, "finalized");
        assert!(recovery.request_response.is_some());

        let action = crate::watcher::ChallengeWatcher::new(store)
            .build_challenge_action(
                note_id,
                &request.public_inputs.request_nullifier,
                [Felt252::ZERO; zkapi_types::MERKLE_DEPTH],
            )
            .expect("challenge watcher should archive real proof artifact");
        assert_eq!(
            action.request_inputs.request_nullifier,
            request.public_inputs.request_nullifier
        );
        assert!(!action.proof_artifact.is_empty());
    }
}
