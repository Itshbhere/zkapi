//! Durable event-to-submission loop for v2 escape challenges.
//!
//! The configured RPC must expose an operator-controlled transaction signer.
//! Keys are never loaded by this service; the sender must be dedicated to it.

use crate::{nullifier_store::NullifierStore, watcher::ChallengeWatcher};
use anyhow::{anyhow, bail, ensure, Context};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use zkapi_proof::compact::RequestVerifier;
use zkapi_types::{
    wire::{Groth16ProofWire, ProofBackendWire},
    Felt252, MERKLE_DEPTH,
};

const ESCAPE_EVENT: &str =
    "EscapeWithdrawalInitiated(uint32,uint256,uint128,address,uint64,uint256)";

#[derive(Clone, Debug)]
pub struct ChallengeServiceConfig {
    pub rpc_url: String,
    pub indexer_url: String,
    pub sender: String,
    pub contract_address: Felt252,
    pub chain_id: u64,
    pub from_block: u64,
    pub confirmations: u64,
    pub checkpoint: PathBuf,
    pub proof_setup_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingChallenge {
    nullifier: Felt252,
    transaction_hash: Option<String>,
    /// Persist before sending so an ambiguous RPC failure retries the same nonce.
    nonce: Option<u64>,
    #[serde(default)]
    deadline_reported: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Checkpoint {
    sender: String,
    chain_id: u64,
    contract_address: Felt252,
    from_block: u64,
    next_block: u64,
    last_block_hash: Option<String>,
    pending: BTreeMap<u32, PendingChallenge>,
}

pub struct ChallengeService {
    config: ChallengeServiceConfig,
    client: reqwest::Client,
    watcher: ChallengeWatcher,
    verifier: RequestVerifier,
    checkpoint: Checkpoint,
}

impl ChallengeService {
    pub fn new(config: ChallengeServiceConfig, store: Arc<NullifierStore>) -> anyhow::Result<Self> {
        ensure!(
            config.contract_address.as_bytes()[..12]
                .iter()
                .all(|byte| *byte == 0),
            "vault address exceeds 20 bytes"
        );
        let sender = config
            .sender
            .strip_prefix("0x")
            .context("sender must be a 0x-prefixed address")?;
        ensure!(
            hex::decode(sender)?.len() == 20,
            "sender must be a 20-byte address"
        );
        let checkpoint: Checkpoint = if config.checkpoint.exists() {
            serde_json::from_slice(&std::fs::read(&config.checkpoint)?)?
        } else {
            Checkpoint {
                sender: config.sender.to_ascii_lowercase(),
                chain_id: config.chain_id,
                contract_address: config.contract_address,
                from_block: config.from_block,
                next_block: config.from_block,
                last_block_hash: None,
                pending: BTreeMap::new(),
            }
        };
        ensure!(
            checkpoint.sender == config.sender.to_ascii_lowercase()
                && checkpoint.chain_id == config.chain_id
                && checkpoint.contract_address == config.contract_address
                && checkpoint.from_block == config.from_block,
            "challenge checkpoint belongs to another deployment"
        );
        Ok(Self {
            verifier: RequestVerifier::load(&config.proof_setup_dir)?,
            config,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            watcher: ChallengeWatcher::new(store),
            checkpoint,
        })
    }

    /// Scan a bounded event range, persist it, then reconcile and submit work.
    /// Errors leave the durable queue intact for the next poll or restart.
    pub async fn poll_once(&mut self) -> anyhow::Result<()> {
        let chain = self.rpc("eth_chainId", json!([])).await?;
        ensure!(
            quantity(&chain)? == self.config.chain_id,
            "RPC chain ID does not match challenge deployment"
        );
        let latest = quantity(&self.rpc("eth_blockNumber", json!([])).await?)?;
        if let Some(expected) = &self.checkpoint.last_block_hash {
            let block = self
                .rpc(
                    "eth_getBlockByNumber",
                    json!([format!("0x{:x}", self.checkpoint.next_block - 1), false]),
                )
                .await?;
            if block.get("hash").and_then(Value::as_str) != Some(expected.as_str()) {
                tracing::warn!("challenge cursor was reorganized; replaying deployment events");
                self.checkpoint.next_block = self.config.from_block;
                self.checkpoint.last_block_hash = None;
                self.persist()?;
            }
        }
        if let Some(safe_head) = latest.checked_sub(self.config.confirmations) {
            if self.checkpoint.next_block <= safe_head {
                let to = safe_head.min(self.checkpoint.next_block.saturating_add(999));
                let before = self
                    .rpc("eth_getBlockByNumber", json!([format!("0x{to:x}"), false]))
                    .await?;
                let before_hash = before["hash"].as_str().context("scan block lacks hash")?;
                let logs = self.rpc("eth_getLogs", json!([{
                    "address": self.vault_address(), "fromBlock": format!("0x{:x}", self.checkpoint.next_block),
                    "toBlock": format!("0x{to:x}"), "topics": [format!("0x{}", hex::encode(Keccak256::digest(ESCAPE_EVENT.as_bytes())))]
                }])).await?;
                let after = self
                    .rpc("eth_getBlockByNumber", json!([format!("0x{to:x}"), false]))
                    .await?;
                let after_hash = after["hash"].as_str().context("scan block lacks hash")?;
                ensure!(
                    before_hash == after_hash,
                    "chain reorganized while reading escape events; retrying range"
                );
                for log in logs.as_array().context("RPC logs must be an array")? {
                    if log.get("removed").and_then(Value::as_bool) == Some(true) {
                        continue;
                    }
                    let topics = log["topics"]
                        .as_array()
                        .context("escape event lacks topics")?;
                    ensure!(topics.len() == 2, "malformed escape event topics");
                    let note = quantity(&topics[1])?;
                    ensure!(note <= u32::MAX as u64, "escape note ID exceeds uint32");
                    let words = words(&log["data"])?;
                    ensure!(words.len() == 5, "malformed escape event data");
                    let nullifier = words[0];
                    let old = self.checkpoint.pending.get(&(note as u32));
                    if old.is_none_or(|old| old.nullifier != nullifier) {
                        self.checkpoint.pending.insert(
                            note as u32,
                            PendingChallenge {
                                nullifier,
                                transaction_hash: None,
                                nonce: None,
                                deadline_reported: false,
                            },
                        );
                    }
                }
                self.checkpoint.last_block_hash = Some(after_hash.to_owned());
                self.checkpoint.next_block = to.saturating_add(1);
                self.persist()?;
            }
        }
        // Each note gets a fresh current-root path. A previous successful
        // challenge can change that root before the next submission.
        let mut notes: Vec<_> = self.checkpoint.pending.keys().copied().collect();
        // Durable ambiguous sends from *previous* polls must run before newly
        // eligible notes, irrespective of note ID or event arrival order.
        notes.sort_by_key(|note| {
            let pending = &self.checkpoint.pending[note];
            (
                pending.nonce.is_none() || pending.transaction_hash.is_some(),
                pending.nonce,
                *note,
            )
        });
        let mut first_error = None;
        for note in notes {
            if let Err(error) = self.reconcile(note).await {
                tracing::error!(note_id = note, error = %error, "escape challenge requires retry");
                if first_error.is_none() {
                    first_error = Some(error);
                }
                // Invalid evidence or a stale path on one note must not starve
                // other notes. An ambiguous send is different: resolve its
                // reserved nonce before asking the signer for another one.
            }
            if self.checkpoint.pending.get(&note).is_some_and(|pending| {
                pending.nonce.is_some() && pending.transaction_hash.is_none()
            }) {
                break;
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn reconcile(&mut self, note: u32) -> anyhow::Result<()> {
        let pending = self.checkpoint.pending[&note].clone();
        let mut query = selector("pendingWithdrawals(uint32)");
        query.extend_from_slice(Felt252::from_u64(note as u64).as_bytes());
        let state = self.call(query.clone()).await?;
        ensure!(state.len() == 6, "malformed pending withdrawal response");
        if state[0] == Felt252::ZERO || state[2] != pending.nullifier {
            // Do not discard an obligation due to an unconfirmed challenge or
            // finalization that a tip reorg could remove without changing the
            // event-scan checkpoint block.
            let latest = quantity(&self.rpc("eth_blockNumber", json!([])).await?)?;
            let Some(confirmed) = latest.checked_sub(self.config.confirmations) else {
                return Ok(());
            };
            let confirmed_state = self.call_at(query, &format!("0x{confirmed:x}")).await?;
            ensure!(
                confirmed_state.len() == 6,
                "malformed confirmed withdrawal response"
            );
            if confirmed_state[0] != Felt252::ZERO && confirmed_state[2] == pending.nullifier {
                return Ok(());
            }
            self.checkpoint.pending.remove(&note);
            self.persist()?;
            return Ok(());
        }
        if self
            .watcher
            .finalized_transcript(&pending.nullifier)
            .is_none()
        {
            return Ok(()); // An outstanding lease can finalize on a later poll.
        }
        let head = self
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await?;
        let deadline = felt_u64(&state[5])?;
        if quantity(&head["timestamp"])? >= deadline {
            if !pending.deadline_reported {
                tracing::error!(note_id = note, nullifier = %pending.nullifier, "MISSED escape challenge deadline");
                self.checkpoint
                    .pending
                    .get_mut(&note)
                    .unwrap()
                    .deadline_reported = true;
                self.persist()?;
            }
            // Keep the obligation until confirmed vault completion, including
            // the possibility of a reorg back before the deadline.
            return Ok(());
        }
        if let Some(hash) = &pending.transaction_hash {
            let receipt = self.rpc("eth_getTransactionReceipt", json!([hash])).await?;
            if receipt.is_null() {
                // A dropped or orphaned transaction can be resubmitted with
                // its durable nonce. A known pending transaction is left alone.
                if !self
                    .rpc("eth_getTransactionByHash", json!([hash]))
                    .await?
                    .is_null()
                {
                    return Ok(());
                }
                self.checkpoint
                    .pending
                    .get_mut(&note)
                    .unwrap()
                    .transaction_hash = None;
            } else {
                let entry = self.checkpoint.pending.get_mut(&note).unwrap();
                entry.transaction_hash = None;
                entry.nonce = None;
                if quantity(&receipt["status"])? == 1 {
                    self.persist()?;
                    return Ok(());
                }
            }
            self.persist()?;
        }
        let response: PathResponse = self
            .client
            .get(format!(
                "{}/v1/tree/notes/{note}/zero-path",
                self.config.indexer_url.trim_end_matches('/')
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            response.note_id == note && response.leaf == Felt252::ZERO,
            "indexer returned the wrong zero slot"
        );
        let siblings: [Felt252; MERKLE_DEPTH] = response
            .siblings
            .try_into()
            .map_err(|_| anyhow!("indexer path must have 32 siblings"))?;
        let root = self.call(selector("currentRoot()")).await?;
        ensure!(
            root.len() == 1
                && zkapi_core::v2::merkle_root(note, &Felt252::ZERO, &siblings) == root[0],
            "indexer path is behind the current vault root"
        );
        let action = self
            .watcher
            .build_challenge_action(note, &pending.nullifier, siblings)
            .map_err(anyhow::Error::msg)?;
        ensure!(
            action.request_inputs.chain_id == self.config.chain_id
                && action.request_inputs.contract_address == self.config.contract_address,
            "archived proof belongs to another deployment"
        );
        let proof = Groth16ProofWire {
            backend: ProofBackendWire::Groth16Bn254,
            proof: base64::engine::general_purpose::STANDARD.encode(&action.proof_artifact),
        };
        ensure!(
            self.verifier.verify(&action.request_inputs, &proof)?,
            "archived challenge proof failed verification"
        );
        let data = format!(
            "0x{}",
            hex::encode(action.calldata().map_err(anyhow::Error::msg)?)
        );
        let mut transaction =
            json!({ "from": self.config.sender, "to": self.vault_address(), "data": data });
        // Simulate against the live contract to check keys, proof, deadline and
        // path together, including races with unrelated tree updates.
        self.rpc("eth_call", json!([transaction, "latest"])).await?;
        let estimated_gas = quantity(&self.rpc("eth_estimateGas", json!([transaction])).await?)?;
        let gas = estimated_gas
            .checked_add(estimated_gas / 5)
            .context("challenge gas estimate overflow")?;
        transaction["gas"] = json!(format!("0x{gas:x}"));
        let chain_nonce = quantity(
            &self
                .rpc(
                    "eth_getTransactionCount",
                    json!([self.config.sender, "pending"]),
                )
                .await?,
        )?;
        let nonce = if let Some(nonce) = self.checkpoint.pending[&note].nonce {
            // A pending count can already include the ambiguous send. Only
            // the mined count permits moving past a previously reserved nonce.
            let mined_nonce = quantity(
                &self
                    .rpc(
                        "eth_getTransactionCount",
                        json!([self.config.sender, "latest"]),
                    )
                    .await?,
            )?;
            if mined_nonce > nonce {
                chain_nonce
            } else {
                nonce
            }
        } else {
            chain_nonce
        };
        self.checkpoint.pending.get_mut(&note).unwrap().nonce = Some(nonce);
        self.persist()?;
        transaction["nonce"] = json!(format!("0x{nonce:x}"));
        let hash = self
            .rpc("eth_sendTransaction", json!([transaction]))
            .await?;
        let hash = hash
            .as_str()
            .context("transaction response lacks hash")?
            .to_owned();
        self.checkpoint
            .pending
            .get_mut(&note)
            .unwrap()
            .transaction_hash = Some(hash.clone());
        self.persist()?;
        tracing::info!(note_id = note, transaction_hash = %hash, "submitted escape challenge");
        Ok(())
    }

    fn vault_address(&self) -> String {
        format!(
            "0x{}",
            hex::encode(&self.config.contract_address.as_bytes()[12..])
        )
    }

    async fn call(&self, data: Vec<u8>) -> anyhow::Result<Vec<Felt252>> {
        self.call_at(data, "latest").await
    }

    async fn call_at(&self, data: Vec<u8>, block: &str) -> anyhow::Result<Vec<Felt252>> {
        words(&self.rpc("eth_call", json!([{ "to": self.vault_address(), "data": format!("0x{}", hex::encode(data)) }, block])).await?)
    }

    async fn rpc(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let response: Value = self
            .client
            .post(&self.config.rpc_url)
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if let Some(error) = response.get("error") {
            bail!("{method}: {error}");
        }
        response
            .get("result")
            .cloned()
            .with_context(|| format!("{method}: missing RPC result"))
    }

    fn persist(&self) -> anyhow::Result<()> {
        let path = &self.config.checkpoint;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        let mut file = std::fs::File::create(&temporary)?;
        std::io::Write::write_all(&mut file, &serde_json::to_vec_pretty(&self.checkpoint)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct PathResponse {
    note_id: u32,
    leaf: Felt252,
    siblings: Vec<Felt252>,
}

fn selector(signature: &str) -> Vec<u8> {
    Keccak256::digest(signature.as_bytes())[..4].to_vec()
}
fn quantity(value: &Value) -> anyhow::Result<u64> {
    let value = value
        .as_str()
        .context("expected hex quantity")?
        .strip_prefix("0x")
        .context("hex quantity lacks prefix")?;
    u64::from_str_radix(value.trim_start_matches('0'), 16)
        .or_else(|error| {
            if value.chars().all(|c| c == '0') {
                Ok(0)
            } else {
                Err(error)
            }
        })
        .context("quantity exceeds uint64")
}
fn felt_u64(value: &Felt252) -> anyhow::Result<u64> {
    ensure!(
        value.as_bytes()[..24].iter().all(|byte| *byte == 0),
        "value exceeds uint64"
    );
    Ok(u64::from_be_bytes(
        value.as_bytes()[24..].try_into().unwrap(),
    ))
}
fn words(value: &Value) -> anyhow::Result<Vec<Felt252>> {
    let raw = value
        .as_str()
        .context("expected ABI bytes")?
        .strip_prefix("0x")
        .context("ABI bytes lack prefix")?;
    let bytes = hex::decode(raw)?;
    ensure!(bytes.len().is_multiple_of(32), "malformed ABI words");
    Ok(bytes
        .chunks_exact(32)
        .map(|chunk| Felt252(chunk.try_into().unwrap()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        routing::{get, post},
        Json, Router,
    };
    use std::sync::Mutex;

    struct MockChain {
        nullifier: Felt252,
        active: bool,
        confirmed_active: bool,
        receipt_failed: bool,
        mined_nonce: u64,
        fail_note_one: bool,
        reorg_during_scan: bool,
        block_hash: String,
        sends: Vec<Value>,
        fail_first_send: bool,
        root: Felt252,
        siblings: [Felt252; MERKLE_DEPTH],
    }
    fn encoded(values: &[Felt252]) -> Value {
        json!(format!(
            "0x{}",
            hex::encode(
                values
                    .iter()
                    .flat_map(|v| v.as_bytes().iter().copied())
                    .collect::<Vec<_>>()
            )
        ))
    }
    async fn rpc(
        State(state): State<Arc<Mutex<MockChain>>>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        let mut state = state.lock().unwrap();
        let method = request["method"].as_str().unwrap();
        let result = match method {
            "eth_chainId" => json!("0x1"),
            "eth_blockNumber" => json!("0x5"),
            "eth_getBlockByNumber" => json!({ "hash": state.block_hash, "timestamp": "0x64" }),
            "eth_getLogs" => {
                if state.reorg_during_scan {
                    state.block_hash = "0xforkB".into();
                    state.reorg_during_scan = false;
                }
                json!([{
                    "topics": [format!("0x{}", hex::encode(Keccak256::digest(ESCAPE_EVENT.as_bytes()))), format!("0x{:064x}", 0)],
                    "data": encoded(&[state.nullifier, Felt252::from_u64(100), Felt252::from_u64(5), Felt252::from_u64(200), state.root])
                }])
            }
            "eth_call" => {
                let data = request["params"][0]["data"].as_str().unwrap();
                if state.fail_note_one
                    && data
                        == format!(
                            "0x{}{}",
                            hex::encode(selector("pendingWithdrawals(uint32)")),
                            hex::encode(Felt252::ONE.as_bytes())
                        )
                {
                    return Json(
                        json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "temporary note-state failure" } }),
                    );
                }
                if data.starts_with(&format!(
                    "0x{}",
                    hex::encode(selector("pendingWithdrawals(uint32)"))
                )) {
                    encoded(&[
                        Felt252::from_u64(if request["params"][1] == "latest" {
                            state.active as u64
                        } else {
                            state.confirmed_active as u64
                        }),
                        Felt252::from_u64(8),
                        state.nullifier,
                        Felt252::from_u64(100),
                        Felt252::from_u64(5),
                        Felt252::from_u64(200),
                    ])
                } else if data == format!("0x{}", hex::encode(selector("currentRoot()"))) {
                    encoded(&[state.root])
                } else {
                    json!("0x")
                }
            }
            "eth_estimateGas" => json!("0x989680"),
            "eth_getTransactionCount" => json!(format!("0x{:x}", state.mined_nonce)),
            "eth_sendTransaction" => {
                state.sends.push(request["params"][0].clone());
                if state.fail_first_send {
                    state.fail_first_send = false;
                    return Json(
                        json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32000, "message": "temporary signer failure" } }),
                    );
                }
                json!("0xsubmitted")
            }
            "eth_getTransactionReceipt" => {
                if state.receipt_failed {
                    state.receipt_failed = false;
                    state.mined_nonce += 1;
                    json!({ "status": "0x0" })
                } else {
                    Value::Null
                }
            }
            "eth_getTransactionByHash" => {
                json!({ "hash": "0xsubmitted", "blockNumber": Value::Null })
            }
            _ => panic!("unexpected RPC method {method}"),
        };
        Json(json!({ "jsonrpc": "2.0", "id": 1, "result": result }))
    }
    async fn path(State(state): State<Arc<Mutex<MockChain>>>) -> Json<Value> {
        let state = state.lock().unwrap();
        Json(json!({"note_id": 0, "leaf": Felt252::ZERO, "siblings": state.siblings.to_vec()}))
    }

    #[tokio::test]
    async fn event_to_submission_retries_after_restart_and_preserves_v2_calldata() {
        let (store, request) = crate::watcher::tests::finalized_v2_request().await;
        let siblings = zkapi_core::v2::zero_hashes()[..MERKLE_DEPTH]
            .try_into()
            .unwrap();
        let root = zkapi_core::v2::merkle_root(0, &Felt252::ZERO, &siblings);
        let chain = Arc::new(Mutex::new(MockChain {
            nullifier: request.public_inputs.request_nullifier,
            active: true,
            confirmed_active: true,
            receipt_failed: false,
            mined_nonce: 3,
            fail_note_one: false,
            reorg_during_scan: true,
            block_hash: "0xforkA".into(),
            sends: vec![],
            fail_first_send: true,
            root,
            siblings,
        }));
        let app = Router::new()
            .route("/", post(rpc))
            .route("/v1/tree/notes/{note}/zero-path", get(path))
            .with_state(chain.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let directory = tempfile::tempdir().unwrap();
        let config = ChallengeServiceConfig {
            rpc_url: url.clone(),
            indexer_url: url,
            sender: "0x0000000000000000000000000000000000000001".into(),
            contract_address: request.public_inputs.contract_address,
            chain_id: 1,
            from_block: 0,
            confirmations: 2,
            checkpoint: directory.path().join("checkpoint.json"),
            proof_setup_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../protocol/setup/v2"),
        };
        let expected = ChallengeWatcher::new(store.clone())
            .build_challenge_action(0, &request.public_inputs.request_nullifier, siblings)
            .unwrap()
            .calldata()
            .unwrap();
        let mut service = ChallengeService::new(config.clone(), store.clone()).unwrap();
        assert!(service
            .poll_once()
            .await
            .unwrap_err()
            .to_string()
            .contains("chain reorganized while reading escape events"));
        assert_eq!(service.checkpoint.next_block, 0);
        assert!(service.checkpoint.pending.is_empty());
        assert!(chain.lock().unwrap().sends.is_empty());
        assert!(service
            .poll_once()
            .await
            .unwrap_err()
            .to_string()
            .contains("temporary signer failure"));
        assert_eq!(service.checkpoint.next_block, 4);
        assert_eq!(service.checkpoint.pending[&0].nonce, Some(3));
        drop(service);
        let mut restarted = ChallengeService::new(config, store).unwrap();
        restarted.poll_once().await.unwrap();
        {
            let chain = chain.lock().unwrap();
            assert_eq!(chain.sends.len(), 2);
            assert_eq!(
                chain.sends[0], chain.sends[1],
                "retry uses the same transaction nonce and payload"
            );
            assert_eq!(
                chain.sends[1]["data"],
                format!("0x{}", hex::encode(expected))
            );
        }
        restarted.poll_once().await.unwrap();
        assert_eq!(
            chain.lock().unwrap().sends.len(),
            2,
            "pending receipt must not cause another send"
        );
        // A reverted challenge refreshes the live path and is resubmitted.
        chain.lock().unwrap().receipt_failed = true;
        restarted.poll_once().await.unwrap();
        assert_eq!(chain.lock().unwrap().sends.len(), 3);
        assert_eq!(chain.lock().unwrap().sends[2]["nonce"], "0x4");
        // A latest-block success alone must not drop the obligation: it can
        // disappear in a tip reorg while the scan checkpoint stays canonical.
        chain.lock().unwrap().active = false;
        restarted.poll_once().await.unwrap();
        assert!(!restarted.checkpoint.pending.is_empty());
        chain.lock().unwrap().confirmed_active = false;
        restarted.poll_once().await.unwrap();
        assert!(restarted.checkpoint.pending.is_empty());

        // A restored ambiguous nonce for a higher note ID must be handled
        // before an earlier note becomes eligible, even if its state RPC fails.
        // Otherwise the new note can steal/replace the reserved transaction.
        {
            let mut chain = chain.lock().unwrap();
            chain.active = true;
            chain.confirmed_active = true;
            chain.fail_note_one = true;
            chain.mined_nonce = 3;
            chain.sends.clear();
        }
        restarted.checkpoint.pending.insert(
            0,
            PendingChallenge {
                nullifier: request.public_inputs.request_nullifier,
                transaction_hash: None,
                nonce: None,
                deadline_reported: false,
            },
        );
        restarted.checkpoint.pending.insert(
            1,
            PendingChallenge {
                nullifier: Felt252::from_u64(999),
                transaction_hash: None,
                nonce: Some(3),
                deadline_reported: false,
            },
        );
        restarted.persist().unwrap();
        assert!(restarted
            .poll_once()
            .await
            .unwrap_err()
            .to_string()
            .contains("temporary note-state failure"));
        assert!(chain.lock().unwrap().sends.is_empty());
        assert!(restarted.checkpoint.pending[&0].nonce.is_none());
        server.abort();
    }
}
