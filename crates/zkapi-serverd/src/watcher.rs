//! Reconstruct v2 escape challenges from finalized or issued-lease evidence.

use std::sync::Arc;

use sha3::{Digest, Keccak256};
use zkapi_types::{Felt252, RequestPublicInputsV2, MERKLE_DEPTH};

use crate::nullifier_store::{NullifierStore, TranscriptRecord};

pub const CHALLENGE_SIGNATURE: &str = "challengeEscapeWithdrawal(uint32,(uint16,uint64,address,uint256,uint256,uint256,uint64,uint128,uint256,uint256,uint256,uint256),bytes,uint256[32])";

/// Fully reconstructed challenge action for the v2 vault.
#[derive(Debug, Clone)]
pub struct ChallengeAction {
    pub note_id: u32,
    pub request_inputs: RequestPublicInputsV2,
    /// Eight big-endian BN254 coordinates, in the archived Groth16 wire order.
    pub proof_artifact: Vec<u8>,
    /// Current zero-slot path, independent of the historical request root.
    pub siblings: [Felt252; MERKLE_DEPTH],
}

impl ChallengeAction {
    /// ABI-encode the exact v2 tuple and archived proof for the vault.
    pub fn calldata(&self) -> Result<Vec<u8>, String> {
        let p = &self.request_inputs;
        if p.protocol_version != 2 {
            return Err("unsupported archived request protocol version".into());
        }
        if p.contract_address.as_bytes()[..12]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err("archived contract address exceeds 20 bytes".into());
        }
        if self.proof_artifact.len() != 256 {
            return Err("archived Groth16 proof must contain eight 32-byte coordinates".into());
        }
        // noteId + static 12-word tuple + bytes offset + static 32-word array.
        let head_words = 1 + 12 + 1 + MERKLE_DEPTH;
        let mut data = Keccak256::digest(CHALLENGE_SIGNATURE.as_bytes())[..4].to_vec();
        for value in [
            Felt252::from_u64(self.note_id as u64),
            Felt252::from_u64(p.protocol_version as u64),
            Felt252::from_u64(p.chain_id),
            p.contract_address,
            p.active_root,
            p.state_signing_key_x,
            p.state_signing_key_y,
            Felt252::from_u64(p.request_time),
            Felt252::from_u128(p.solvency_bound),
            p.request_nullifier,
            p.authorization_tag,
            p.anonymous_commitment_x,
            p.anonymous_commitment_y,
            Felt252::from_u64((head_words * 32) as u64),
        ] {
            data.extend_from_slice(value.as_bytes());
        }
        for sibling in self.siblings {
            data.extend_from_slice(sibling.as_bytes());
        }
        data.extend_from_slice(Felt252::from_u64(256).as_bytes());
        data.extend_from_slice(&self.proof_artifact);
        Ok(data)
    }
}

pub struct ChallengeWatcher {
    store: Arc<NullifierStore>,
}

impl ChallengeWatcher {
    pub fn new(store: Arc<NullifierStore>) -> Self {
        Self { store }
    }

    /// Finalized usage is not required once a runtime key may have been given
    /// to the user. Its exact accepted proof is durable before activation.
    /// Provisioning alone is insufficient: a key may never have been returned.
    pub fn challenge_transcript(&self, nullifier: &Felt252) -> Option<TranscriptRecord> {
        use crate::nullifier_store::api_request_binding;
        use base64::Engine;
        let mut record = self.store.lookup_by_nullifier(nullifier)?;
        if record.status == zkapi_types::NullifierStatus::Finalized {
            return Some(record);
        }
        if record.status != zkapi_types::NullifierStatus::Reserved
            || record.reservation_kind != "openrouter_lease"
        {
            return None;
        }
        let id = record.client_request_id.as_deref()?;
        let lease = self.store.lookup_openrouter_lease(id)?;
        let request = &lease.api_request;
        // Retirement has already stopped or is stopping future key use, but
        // the issued authorization remains consumed until settlement completes.
        if !matches!(
            lease.status.as_str(),
            "active" | "retiring" | "disabled" | "revoking"
        ) || lease.key_hash.is_none()
            || lease.client_request_id != id
            || request.client_request_id != id
            || lease.request_nullifier != *nullifier
            || request.public_inputs.request_nullifier != *nullifier
            || record.payload_hash != Some(request.payload_hash)
            || record.api_request_binding.as_deref()
                != Some(api_request_binding(request).ok()?.as_str())
            || request.proof.backend != zkapi_types::wire::ProofBackendWire::Groth16Bn254
        {
            return None;
        }
        record.request_inputs_json = Some(serde_json::to_string(&request.public_inputs).ok()?);
        record.proof_blob = Some(
            base64::engine::general_purpose::STANDARD
                .decode(&request.proof.proof)
                .ok()?,
        );
        Some(record)
    }

    pub fn build_challenge_action(
        &self,
        note_id: u32,
        nullifier: &Felt252,
        siblings: [Felt252; MERKLE_DEPTH],
    ) -> Result<ChallengeAction, String> {
        let record = self
            .challenge_transcript(nullifier)
            .ok_or_else(|| "no finalized or issued-lease evidence for nullifier".to_string())?;
        let request_inputs: RequestPublicInputsV2 = serde_json::from_str(
            &record
                .request_inputs_json
                .ok_or("missing archived request inputs")?,
        )
        .map_err(|error| format!("invalid archived v2 request inputs: {error}"))?;
        if request_inputs.request_nullifier != *nullifier {
            return Err("archived request nullifier mismatch".to_string());
        }
        let action = ChallengeAction {
            note_id,
            request_inputs,
            proof_artifact: record.proof_blob.ok_or("missing archived proof blob")?,
            siblings,
        };
        // Fail at the compatibility boundary, before submitting malformed calldata.
        action.calldata()?;
        Ok(action)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{config::ServerConfig, processor::RequestProcessor, signer::ServerSigner};
    use base64::Engine;
    use zkapi_core::v2 as core;
    use zkapi_proof::compact::{balance_commitment, RequestProver, RequestWitnessData};
    use zkapi_types::wire::ApiRequestV2;
    use zkapi_types::{canonical_payload_hash, canonical_request_context};

    /// A real v2 proof accepted and archived by the active processor, rather
    /// than a hand-constructed transcript that could drift from its writer.
    pub(crate) async fn finalized_v2_request() -> (Arc<NullifierStore>, ApiRequestV2) {
        static FIXTURE: tokio::sync::OnceCell<(Arc<NullifierStore>, ApiRequestV2)> =
            tokio::sync::OnceCell::const_new();
        FIXTURE
            .get_or_init(|| async {
                let setup_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../protocol/setup/v2");
                let signer = Arc::new(ServerSigner::new(
                    &Felt252::from_u64(11),
                    &Felt252::from_u64(12),
                ));
                let key = signer.state_public_key();
                let secret = Felt252::from_u64(42);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let expiry = now + 86_400;
                let siblings = core::zero_hashes()[..MERKLE_DEPTH].try_into().unwrap();
                let registration = core::registration_commitment(&secret);
                let leaf = core::note_leaf(0, &registration, 100, expiry);
                let root = core::merkle_root(0, &leaf, &siblings);
                let nullifier = core::nullifier(&secret, &Felt252::ONE);
                let test_config = ServerConfig {
                    native_billing: Some(crate::test_support::native_config()),
                    openrouter_leases: Some(crate::test_support::lease_config()),
                    ..Default::default()
                };
                let payload = crate::test_support::lease_payload(&test_config, now);
                let payload_hash = canonical_payload_hash(payload.as_bytes());
                let request_context = canonical_request_context("challenge-v2", &payload_hash);
                let blind = Felt252::from_u64(17);
                let anonymous = balance_commitment(100, &blind, &leaf);
                let public = RequestPublicInputsV2 {
                    protocol_version: 2,
                    chain_id: 1,
                    contract_address: Felt252::from_u64(0xdead),
                    active_root: root,
                    state_signing_key_x: key.x,
                    state_signing_key_y: key.y,
                    request_time: now,
                    solvency_bound: 1,
                    request_nullifier: nullifier,
                    authorization_tag: core::authorization_tag(&nullifier, &request_context),
                    anonymous_commitment_x: anonymous.x,
                    anonymous_commitment_y: anonymous.y,
                };
                let proof = RequestProver::load(&setup_dir)
                    .unwrap()
                    .prove(
                        &public,
                        RequestWitnessData {
                            secret,
                            request_context,
                            note_id: 0,
                            deposit_amount: 100,
                            expiry,
                            merkle_siblings: siblings,
                            current_balance: 100,
                            current_blinding: Felt252::ZERO,
                            rerandomization: blind,
                            current_anchor: Felt252::ONE,
                            is_genesis: true,
                            state_signature: None,
                        },
                    )
                    .unwrap();
                let request = ApiRequestV2 {
                    client_request_id: "challenge-v2".into(),
                    payload_hash,
                    payload,
                    public_inputs: public.clone(),
                    proof,
                };
                let store = Arc::new(NullifierStore::in_memory().unwrap());
                let processor = RequestProcessor::try_new(
                    ServerConfig {
                        contract_address: public.contract_address,
                        request_charge_cap: 1,
                        proof_setup_dir: setup_dir.to_string_lossy().into_owned(),
                        ..test_config
                    },
                    store.clone(),
                    signer,
                    root,
                )
                .unwrap();
                processor.finalize_test_lease(&request, 1).unwrap();
                (store, request)
            })
            .await
            .clone()
    }

    #[tokio::test]
    async fn active_lease_can_be_challenged_without_receipt_but_provisioning_or_changed_binding_cannot(
    ) {
        let (_, request) = finalized_v2_request().await;
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        store.reserve_openrouter_lease(&request).unwrap();
        store
            .create_openrouter_lease(&request, "oa_org", 1, 2, 3, 1.0)
            .unwrap();
        let watcher = ChallengeWatcher::new(store.clone());
        let nullifier = request.public_inputs.request_nullifier;
        assert!(
            watcher.challenge_transcript(&nullifier).is_none(),
            "provisioning does not establish that a key was returned"
        );
        store
            .activate_openrouter_lease(&request.client_request_id, "issued-key-hash")
            .unwrap();
        let action = watcher
            .build_challenge_action(0, &nullifier, [Felt252::ZERO; MERKLE_DEPTH])
            .unwrap();
        assert_eq!(
            action.request_inputs.active_root,
            request.public_inputs.active_root
        );
        assert_eq!(
            action.proof_artifact,
            base64::engine::general_purpose::STANDARD
                .decode(&request.proof.proof)
                .unwrap()
        );
        let reservation = store.lookup_by_nullifier(&nullifier).unwrap();
        assert_eq!(reservation.status, zkapi_types::NullifierStatus::Reserved);
        assert!(reservation.response_hash.is_none());
        assert!(
            reservation.proof_blob.is_none(),
            "read-only reconstruction must not finalize usage"
        );

        let changed_store = Arc::new(NullifierStore::in_memory().unwrap());
        changed_store.reserve_openrouter_lease(&request).unwrap();
        let mut changed = request.clone();
        changed.public_inputs.active_root = Felt252::from_u64(999);
        changed_store
            .create_openrouter_lease(&changed, "oa_org", 1, 2, 3, 1.0)
            .unwrap();
        changed_store
            .activate_openrouter_lease(&changed.client_request_id, "issued-key-hash")
            .unwrap();
        assert!(ChallengeWatcher::new(changed_store)
            .challenge_transcript(&nullifier)
            .is_none());
    }

    #[tokio::test]
    async fn direct_key_retirement_preserves_challenge_evidence_until_finalization() {
        let (_, request) = finalized_v2_request().await;
        let store = Arc::new(NullifierStore::in_memory().unwrap());
        store.reserve_openrouter_lease(&request).unwrap();
        store
            .create_openrouter_lease(&request, "openrouter", 1, 2, 3, 1.0)
            .unwrap();
        store
            .activate_openrouter_lease(&request.client_request_id, "issued-key-hash")
            .unwrap();
        let watcher = ChallengeWatcher::new(store.clone());
        let nullifier = request.public_inputs.request_nullifier;
        for (from, to) in [
            ("active", "retiring"),
            ("retiring", "disabled"),
            ("disabled", "revoking"),
        ] {
            store
                .advance_openrouter_retirement(&request.client_request_id, from, to, 0, None)
                .unwrap();
            let action = watcher
                .build_challenge_action(0, &nullifier, [Felt252::ZERO; MERKLE_DEPTH])
                .unwrap_or_else(|error| panic!("{to} must remain challengeable: {error}"));
            assert_eq!(
                action.request_inputs.active_root,
                request.public_inputs.active_root
            );
            assert_eq!(
                action.proof_artifact,
                base64::engine::general_purpose::STANDARD
                    .decode(&request.proof.proof)
                    .unwrap()
            );
            let reservation = store.lookup_by_nullifier(&nullifier).unwrap();
            assert_eq!(reservation.status, zkapi_types::NullifierStatus::Reserved);
            assert!(reservation.response_hash.is_none());
        }
    }

    #[tokio::test]
    async fn active_v2_processor_transcript_builds_challenge_calldata() {
        let (store, request) = finalized_v2_request().await;
        let nullifier = request.public_inputs.request_nullifier;
        let watcher = ChallengeWatcher::new(store);
        let action = watcher
            .build_challenge_action(0, &nullifier, [Felt252::ZERO; MERKLE_DEPTH])
            .unwrap();
        assert_eq!(
            serde_json::to_value(&action.request_inputs).unwrap(),
            serde_json::to_value(&request.public_inputs).unwrap()
        );
        assert_eq!(
            action.proof_artifact,
            base64::engine::general_purpose::STANDARD
                .decode(&request.proof.proof)
                .unwrap()
        );
        let calldata = action.calldata().unwrap();
        assert_eq!(&calldata[..4], &[0x8a, 0x43, 0x46, 0x7c]);
        assert_eq!(calldata.len(), 4 + (46 + 1 + 8) * 32);
        assert_eq!(
            &calldata[4 + 4 * 32..4 + 5 * 32],
            request.public_inputs.active_root.as_bytes()
        );
        assert_eq!(&calldata[4 + 9 * 32..4 + 10 * 32], nullifier.as_bytes());
        assert_eq!(
            &calldata[4 + 13 * 32..4 + 14 * 32],
            Felt252::from_u64(46 * 32).as_bytes()
        );
        assert_eq!(&calldata[4 + 47 * 32..], action.proof_artifact);
        let mut malformed = action.clone();
        malformed.proof_artifact.pop();
        assert!(malformed.calldata().is_err());
        malformed = action;
        malformed.request_inputs.protocol_version = 1;
        assert!(malformed.calldata().is_err());
    }
}
