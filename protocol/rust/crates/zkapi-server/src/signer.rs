//! Server signing module that manages XMSS keypairs.
//!
//! The server maintains two XMSS trees:
//! - state keypair: signs state transitions (next commitment + anchor)
//! - clear keypair: signs clearance messages for mutual close

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};
use zkapi_core::poseidon::FieldElement;

use zkapi_crypto::xmss::XmssKeypair;
use zkapi_types::{Felt252, XmssSignature};

use crate::error::ServerError;

/// Server-side signer holding both state and clearance XMSS keypairs.
pub struct ServerSigner {
    state_keypair: XmssKeypair,
    clear_keypair: XmssKeypair,
    state_epoch: u32,
    clear_epoch: u32,
    durable_state: Option<Arc<SignerStateStore>>,
    operation_lock: Mutex<()>,
}

impl ServerSigner {
    /// Create a new server signer from seeds and an epoch number.
    ///
    /// `state_seed` is used to derive the state-signing XMSS tree.
    /// `clear_seed` is used to derive the clearance-signing XMSS tree.
    /// `epoch` is the initial epoch number assigned to both trees.
    pub fn new(state_seed: FieldElement, clear_seed: FieldElement, epoch: u32) -> Self {
        Self::with_height(state_seed, clear_seed, epoch, zkapi_types::XMSS_TREE_HEIGHT)
    }

    /// Create a signer with a custom tree height (for testing).
    pub fn with_height(
        state_seed: FieldElement,
        clear_seed: FieldElement,
        epoch: u32,
        height: usize,
    ) -> Self {
        let state_keypair = XmssKeypair::generate_with_height(&state_seed, height);
        let clear_keypair = XmssKeypair::generate_with_height(&clear_seed, height);
        Self {
            state_keypair,
            clear_keypair,
            state_epoch: epoch,
            clear_epoch: epoch,
            durable_state: None,
            operation_lock: Mutex::new(()),
        }
    }

    /// Create a signer whose leaf counters are persisted in SQLite.
    ///
    /// A leaf reservation increments the stored counter before message
    /// construction/signing, so a crash after reservation burns the leaf rather
    /// than risking WOTS reuse on restart.
    pub fn with_height_durable<P: AsRef<Path>>(
        state_seed: FieldElement,
        clear_seed: FieldElement,
        epoch: u32,
        height: usize,
        db_path: P,
    ) -> Result<Self, ServerError> {
        let state_keypair = XmssKeypair::generate_with_height(&state_seed, height);
        let clear_keypair = XmssKeypair::generate_with_height(&clear_seed, height);
        let durable_state = SignerStateStore::new(
            db_path,
            epoch,
            state_keypair.root_felt(),
            epoch,
            clear_keypair.root_felt(),
        )?;
        Ok(Self {
            state_keypair,
            clear_keypair,
            state_epoch: epoch,
            clear_epoch: epoch,
            durable_state: Some(Arc::new(durable_state)),
            operation_lock: Mutex::new(()),
        })
    }

    /// Sign a state message using the state XMSS keypair.
    ///
    /// Returns the signature with the correct epoch set, plus the leaf index used.
    pub fn sign_state(&self, message: &Felt252) -> Result<(XmssSignature, u32), ServerError> {
        let _lock = self
            .operation_lock
            .lock()
            .map_err(|e| ServerError::Internal(format!("signer operation lock poisoned: {}", e)))?;
        let (mut sig, leaf_index) = if let Some(store) = &self.durable_state {
            let leaf_index = store.reserve_leaf(
                SignerTree::State,
                self.state_epoch,
                &self.state_root(),
                self.state_keypair.capacity(),
            )?;
            let sig = self
                .state_keypair
                .sign_reserved(leaf_index, message)
                .ok_or(ServerError::CapacityExhausted)?;
            (sig, leaf_index)
        } else {
            self.state_keypair
                .sign(message)
                .ok_or(ServerError::CapacityExhausted)?
        };
        sig.epoch = self.state_epoch;
        Ok((sig, leaf_index))
    }

    /// Reserve the next state leaf, build the state message from that exact
    /// leaf index, and sign it under one signer lock.
    pub fn sign_next_state<F>(
        &self,
        build_message: F,
    ) -> Result<(XmssSignature, u32, Felt252), ServerError>
    where
        F: FnOnce(u32) -> Felt252,
    {
        let _lock = self
            .operation_lock
            .lock()
            .map_err(|e| ServerError::Internal(format!("signer operation lock poisoned: {}", e)))?;
        let (mut sig, leaf_index, message) = if let Some(store) = &self.durable_state {
            let leaf_index = store.reserve_leaf(
                SignerTree::State,
                self.state_epoch,
                &self.state_root(),
                self.state_keypair.capacity(),
            )?;
            let message = build_message(leaf_index);
            let sig = self
                .state_keypair
                .sign_reserved(leaf_index, &message)
                .ok_or(ServerError::CapacityExhausted)?;
            (sig, leaf_index, message)
        } else {
            self.state_keypair
                .sign_with_index(build_message)
                .ok_or(ServerError::CapacityExhausted)?
        };
        sig.epoch = self.state_epoch;
        Ok((sig, leaf_index, message))
    }

    /// Sign a clearance message using the clearance XMSS keypair.
    ///
    /// Returns the signature with the correct epoch set, plus the leaf index used.
    pub fn sign_clearance(&self, message: &Felt252) -> Result<(XmssSignature, u32), ServerError> {
        let _lock = self
            .operation_lock
            .lock()
            .map_err(|e| ServerError::Internal(format!("signer operation lock poisoned: {}", e)))?;
        let (mut sig, leaf_index) = if let Some(store) = &self.durable_state {
            let leaf_index = store.reserve_leaf(
                SignerTree::Clear,
                self.clear_epoch,
                &self.clear_root(),
                self.clear_keypair.capacity(),
            )?;
            let sig = self
                .clear_keypair
                .sign_reserved(leaf_index, message)
                .ok_or(ServerError::CapacityExhausted)?;
            (sig, leaf_index)
        } else {
            self.clear_keypair
                .sign(message)
                .ok_or(ServerError::CapacityExhausted)?
        };
        sig.epoch = self.clear_epoch;
        Ok((sig, leaf_index))
    }

    /// Get the state XMSS tree root as a Felt252.
    pub fn state_root(&self) -> Felt252 {
        self.state_keypair.root_felt()
    }

    /// Get the clearance XMSS tree root as a Felt252.
    pub fn clear_root(&self) -> Felt252 {
        self.clear_keypair.root_felt()
    }

    /// Get the current state epoch.
    pub fn epoch(&self) -> u32 {
        self.state_epoch
    }

    /// Get the current clear epoch.
    pub fn clear_epoch(&self) -> u32 {
        self.clear_epoch
    }

    /// Check remaining state signatures.
    pub fn state_remaining(&self) -> u32 {
        if let Some(store) = &self.durable_state {
            return store
                .next_leaf(SignerTree::State)
                .ok()
                .and_then(|next| self.state_keypair.capacity().checked_sub(next))
                .unwrap_or(0);
        }
        self.state_keypair.remaining()
    }

    /// Peek the next state-signature leaf index.
    pub fn state_next_index(&self) -> u32 {
        if let Some(store) = &self.durable_state {
            return store.next_leaf(SignerTree::State).unwrap_or(0);
        }
        self.state_keypair.next_index()
    }

    /// Check remaining clearance signatures.
    pub fn clear_remaining(&self) -> u32 {
        if let Some(store) = &self.durable_state {
            return store
                .next_leaf(SignerTree::Clear)
                .ok()
                .and_then(|next| self.clear_keypair.capacity().checked_sub(next))
                .unwrap_or(0);
        }
        self.clear_keypair.remaining()
    }

    /// Peek the next clearance-signature leaf index.
    pub fn clear_next_index(&self) -> u32 {
        if let Some(store) = &self.durable_state {
            return store.next_leaf(SignerTree::Clear).unwrap_or(0);
        }
        self.clear_keypair.next_index()
    }
}

#[derive(Clone, Copy)]
enum SignerTree {
    State,
    Clear,
}

impl SignerTree {
    fn as_str(self) -> &'static str {
        match self {
            SignerTree::State => "state",
            SignerTree::Clear => "clear",
        }
    }
}

/// Durable signer leaf reservation state.
pub struct SignerStateStore {
    conn: Mutex<Connection>,
}

impl SignerStateStore {
    pub fn new<P: AsRef<Path>>(
        path: P,
        state_epoch: u32,
        state_root: Felt252,
        clear_epoch: u32,
        clear_root: Felt252,
    ) -> Result<Self, ServerError> {
        let conn = Connection::open(path)
            .map_err(|e| ServerError::Database(format!("failed to open signer db: {}", e)))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS signer_state (
                tree_kind TEXT PRIMARY KEY,
                epoch INTEGER NOT NULL,
                root TEXT NOT NULL,
                next_leaf INTEGER NOT NULL
            );",
        )
        .map_err(|e| ServerError::Database(format!("failed to create signer_state: {}", e)))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.ensure_tree(SignerTree::State, state_epoch, &state_root)?;
        store.ensure_tree(SignerTree::Clear, clear_epoch, &clear_root)?;
        Ok(store)
    }

    fn ensure_tree(&self, tree: SignerTree, epoch: u32, root: &Felt252) -> Result<(), ServerError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ServerError::Database(format!("signer_state lock poisoned: {}", e)))?;
        let row: Option<(i64, String)> = conn
            .query_row(
                "SELECT epoch, root FROM signer_state WHERE tree_kind = ?1",
                params![tree.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| ServerError::Database(format!("signer_state lookup failed: {}", e)))?;
        match row {
            Some((stored_epoch, stored_root))
                if stored_epoch == epoch as i64 && stored_root == root.to_hex() =>
            {
                Ok(())
            }
            Some((stored_epoch, stored_root)) => Err(ServerError::Database(format!(
                "signer_state {} mismatch: db has epoch={} root={}, config has epoch={} root={}",
                tree.as_str(),
                stored_epoch,
                stored_root,
                epoch,
                root
            ))),
            None => {
                conn.execute(
                    "INSERT INTO signer_state (tree_kind, epoch, root, next_leaf)
                     VALUES (?1, ?2, ?3, 0)",
                    params![tree.as_str(), epoch as i64, root.to_hex()],
                )
                .map_err(|e| ServerError::Database(format!("signer_state insert failed: {}", e)))?;
                Ok(())
            }
        }
    }

    fn next_leaf(&self, tree: SignerTree) -> Result<u32, ServerError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| ServerError::Database(format!("signer_state lock poisoned: {}", e)))?;
        let next: i64 = conn
            .query_row(
                "SELECT next_leaf FROM signer_state WHERE tree_kind = ?1",
                params![tree.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| ServerError::Database(format!("signer_state lookup failed: {}", e)))?;
        u32::try_from(next)
            .map_err(|_| ServerError::Database("signer_state next_leaf out of range".to_string()))
    }

    fn reserve_leaf(
        &self,
        tree: SignerTree,
        epoch: u32,
        root: &Felt252,
        capacity: u32,
    ) -> Result<u32, ServerError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| ServerError::Database(format!("signer_state lock poisoned: {}", e)))?;
        let tx = conn
            .transaction()
            .map_err(|e| ServerError::Database(format!("signer_state tx failed: {}", e)))?;
        let row: (i64, String, i64) = tx
            .query_row(
                "SELECT epoch, root, next_leaf FROM signer_state WHERE tree_kind = ?1",
                params![tree.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| {
                ServerError::Database(format!("signer_state reserve lookup failed: {}", e))
            })?;
        if row.0 != epoch as i64 || row.1 != root.to_hex() {
            return Err(ServerError::Database(format!(
                "signer_state {} changed: db has epoch={} root={}, signer has epoch={} root={}",
                tree.as_str(),
                row.0,
                row.1,
                epoch,
                root
            )));
        }
        let next = u32::try_from(row.2).map_err(|_| {
            ServerError::Database("signer_state next_leaf out of range".to_string())
        })?;
        if next >= capacity {
            return Err(ServerError::CapacityExhausted);
        }
        tx.execute(
            "UPDATE signer_state SET next_leaf = ?1 WHERE tree_kind = ?2",
            params![(next + 1) as i64, tree.as_str()],
        )
        .map_err(|e| ServerError::Database(format!("signer_state reserve update failed: {}", e)))?;
        tx.commit()
            .map_err(|e| ServerError::Database(format!("signer_state tx commit failed: {}", e)))?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zkapi_crypto::xmss::XmssVerifier;

    fn temp_db(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("zkapi_signer_{name}_{nonce}.db"))
    }

    #[test]
    fn sign_next_state_reserves_unique_leaf_and_signs_matching_message() {
        let signer = Arc::new(ServerSigner::with_height(
            FieldElement::from(123u64),
            FieldElement::from(456u64),
            9,
            4,
        ));
        let seen = Arc::new(Mutex::new(Vec::new()));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let signer = Arc::clone(&signer);
            let seen = Arc::clone(&seen);
            handles.push(thread::spawn(move || {
                signer
                    .sign_next_state(|leaf| {
                        seen.lock().unwrap().push(leaf);
                        Felt252::from_u64(10_000 + leaf as u64)
                    })
                    .unwrap()
            }));
        }

        let root = signer.state_root();
        let mut signed = Vec::new();
        for handle in handles {
            let (sig, leaf, msg) = handle.join().unwrap();
            assert_eq!(sig.epoch, 9);
            assert_eq!(sig.leaf_index, leaf);
            assert_eq!(msg, Felt252::from_u64(10_000 + leaf as u64));
            assert!(XmssVerifier::verify_for_height(&root, &msg, &sig, 4));
            signed.push(leaf);
        }
        signed.sort_unstable();
        assert_eq!(signed, (0u32..8).collect::<Vec<_>>());

        let mut observed = seen.lock().unwrap().clone();
        observed.sort_unstable();
        assert_eq!(observed, signed);
    }

    #[test]
    fn durable_signer_restart_does_not_reuse_leaf() {
        let db = temp_db("restart");
        let state_seed = FieldElement::from(123u64);
        let clear_seed = FieldElement::from(456u64);
        let first = ServerSigner::with_height_durable(state_seed, clear_seed, 3, 4, &db).unwrap();
        let (sig0, leaf0, msg0) = first
            .sign_next_state(|leaf| Felt252::from_u64(20_000 + leaf as u64))
            .unwrap();
        assert_eq!(leaf0, 0);
        assert_eq!(sig0.leaf_index, 0);
        assert_eq!(msg0, Felt252::from_u64(20_000));
        drop(first);

        let restarted =
            ServerSigner::with_height_durable(state_seed, clear_seed, 3, 4, &db).unwrap();
        let (sig1, leaf1, msg1) = restarted
            .sign_next_state(|leaf| Felt252::from_u64(20_000 + leaf as u64))
            .unwrap();
        assert_eq!(leaf1, 1);
        assert_eq!(sig1.leaf_index, 1);
        assert_eq!(msg1, Felt252::from_u64(20_001));
        let _ = fs::remove_file(db);
    }

    #[test]
    fn durable_reserved_but_unsigned_leaf_is_burned() {
        let db = temp_db("crash");
        let state_seed = FieldElement::from(777u64);
        let clear_seed = FieldElement::from(888u64);
        let signer = ServerSigner::with_height_durable(state_seed, clear_seed, 5, 4, &db).unwrap();
        let store = signer.durable_state.as_ref().unwrap();
        let burned = store
            .reserve_leaf(SignerTree::State, signer.epoch(), &signer.state_root(), 16)
            .unwrap();
        assert_eq!(burned, 0);
        drop(signer);

        let restarted =
            ServerSigner::with_height_durable(state_seed, clear_seed, 5, 4, &db).unwrap();
        let (sig, leaf, msg) = restarted
            .sign_next_state(|leaf| Felt252::from_u64(30_000 + leaf as u64))
            .unwrap();
        assert_eq!(leaf, 1);
        assert_eq!(sig.leaf_index, 1);
        assert_eq!(msg, Felt252::from_u64(30_001));
        let _ = fs::remove_file(db);
    }
}
