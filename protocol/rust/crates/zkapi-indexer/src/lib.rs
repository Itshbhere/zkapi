//! Indexer: mirrors contract events into a local Merkle tree view.
//!
//! The indexer maintains a full copy of the on-chain Merkle tree by consuming
//! contract events in block order. It serves sibling paths and the latest root
//! to both the client and server.
//!
//! The indexer is NOT trusted. Incorrect paths only cause proof or tx failure.

pub mod events;
pub mod service;
pub mod tree_mirror;

pub use service::IndexerService;
pub use tree_mirror::TreeMirror;
