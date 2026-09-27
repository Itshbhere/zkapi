//! Operator-run v2 challenge daemon; see docs/challenge-service.md.
use anyhow::Context;
use clap::Parser;
use std::{path::PathBuf, sync::Arc, time::Duration};
use zkapi_serverd::{
    challenge_service::{ChallengeService, ChallengeServiceConfig},
    nullifier_store::NullifierStore,
};
use zkapi_types::Felt252;

#[derive(Parser)]
#[command(
    about = "Challenge consumed-state escape withdrawals using the v2 server transcript database"
)]
struct Args {
    /// Operator-controlled RPC supporting eth_sendTransaction for --sender.
    #[arg(long, env = "ZKAPI_CHALLENGE_RPC_URL")]
    rpc_url: String,
    #[arg(long, env = "ZKAPI_CHALLENGE_INDEXER_URL")]
    indexer_url: String,
    /// Dedicated gas-paying account managed by the configured signer RPC.
    #[arg(long, env = "ZKAPI_CHALLENGE_SENDER")]
    sender: String,
    #[arg(long)]
    contract_address: String,
    #[arg(long)]
    chain_id: u64,
    /// Vault deployment block; never start at the current head on a restart.
    #[arg(long)]
    from_block: u64,
    #[arg(long, default_value_t = 2)]
    confirmations: u64,
    #[arg(long)]
    db_path: PathBuf,
    #[arg(long)]
    checkpoint: PathBuf,
    #[arg(long)]
    proof_setup_dir: PathBuf,
    #[arg(long, default_value_t = 1000)]
    poll_interval_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    anyhow::ensure!(args.poll_interval_ms > 0, "poll interval must be positive");
    anyhow::ensure!(
        args.db_path.is_file(),
        "server transcript database does not exist"
    );
    if let Some(parent) = args
        .checkpoint
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(args.checkpoint.with_extension("lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .context("another challenge daemon holds this checkpoint")?;
    let config = ChallengeServiceConfig {
        rpc_url: args.rpc_url,
        indexer_url: args.indexer_url,
        sender: args.sender,
        contract_address: Felt252::from_hex(&args.contract_address).map_err(anyhow::Error::msg)?,
        chain_id: args.chain_id,
        from_block: args.from_block,
        confirmations: args.confirmations,
        checkpoint: args.checkpoint,
        proof_setup_dir: args.proof_setup_dir,
    };
    let store = Arc::new(NullifierStore::new(&args.db_path)?);
    let mut service = ChallengeService::new(config, store)?;
    let mut ticker = tokio::time::interval(Duration::from_millis(args.poll_interval_ms));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = ticker.tick() => if let Err(error) = service.poll_once().await {
                tracing::error!(error = %error, "challenge poll failed; durable work will retry");
            }
        }
    }
}
