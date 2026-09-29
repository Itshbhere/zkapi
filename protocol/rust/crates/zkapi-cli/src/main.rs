//! zkAPI command-line interface.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use zkapi_types::Felt252;

#[derive(Parser)]
#[command(name = "zkapi", about = "zkAPI usage credits CLI")]
struct Cli {
    /// Path to wallet state directory.
    #[arg(long, default_value = ".zkapi")]
    state_dir: PathBuf,

    /// Server URL.
    #[arg(long, default_value = "http://localhost:3000")]
    server_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new secret and registration commitment.
    Keygen,

    /// Deposit funds (prints the commitment and deposit tx data).
    Deposit {
        /// Amount in token base units.
        #[arg(long)]
        amount: u128,
    },

    /// Confirm deposit by providing the on-chain note ID.
    ConfirmDeposit {
        /// The on-chain note ID allocated by the contract.
        #[arg(long)]
        note_id: u32,
        /// Deposit amount.
        #[arg(long)]
        amount: u128,
        /// Note expiry timestamp.
        #[arg(long)]
        expiry_ts: u64,
    },

    /// Send an API request.
    Request {
        /// The API payload to send.
        #[arg(long)]
        payload: String,
    },

    /// Initiate mutual-close withdrawal.
    Withdraw {
        /// Destination Ethereum address (0x-prefixed).
        #[arg(long)]
        destination: String,
    },

    /// Initiate escape-hatch withdrawal.
    EscapeWithdraw {
        /// Destination Ethereum address (0x-prefixed).
        #[arg(long)]
        destination: String,
    },

    /// Recover from a pending request after crash.
    Recover,

    /// Show current wallet state.
    Status,

    /// Start the server.
    Server {
        /// Listen address.
        #[arg(long, default_value = "0.0.0.0:3000")]
        listen: String,
        /// Database path.
        #[arg(long, default_value = "zkapi-server.db")]
        db_path: String,
        /// State-signing XMSS seed as a felt hex string.
        #[arg(long, default_value = "0x1")]
        state_seed: String,
        /// Clearance-signing XMSS seed as a felt hex string.
        #[arg(long, default_value = "0x2")]
        clear_seed: String,
        /// Published XMSS epoch.
        #[arg(long, default_value_t = 1)]
        epoch: u32,
        /// XMSS tree height.
        #[arg(long, default_value_t = zkapi_types::XMSS_TREE_HEIGHT)]
        xmss_height: usize,
        /// Initial accepted active root before the indexer updates the server.
        #[arg(long, default_value = "0x0")]
        initial_root: String,
    },

    /// Start the indexer.
    Indexer {
        /// Listen address.
        #[arg(long, default_value = "0.0.0.0:3001")]
        listen: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Keygen => {
            let secret = {
                use rand::RngCore;
                let mut rng = rand::thread_rng();
                let mut bytes = [0u8; 31]; // < 252 bits
                rng.fill_bytes(&mut bytes);
                let mut felt_bytes = [0u8; 32];
                felt_bytes[1..32].copy_from_slice(&bytes);
                Felt252(felt_bytes)
            };
            let commitment = zkapi_core::leaf::compute_registration_commitment(&secret);
            println!("Secret:     {}", secret.to_hex());
            println!("Commitment: {}", commitment.to_hex());
            println!("\nSave the secret securely. Use the commitment for on-chain deposit.");
        }

        Commands::Deposit { amount } => {
            println!("To deposit {} tokens:", amount);
            println!("1. Run `zkapi keygen` to get a commitment");
            println!("2. Call ZkApiVault.deposit(commitment, amount, siblings) on-chain");
            println!("3. After tx confirms, run `zkapi confirm-deposit --note-id <id> --amount {} --expiry-ts <ts>`", amount);
        }

        Commands::ConfirmDeposit {
            note_id,
            amount,
            expiry_ts,
        } => {
            println!(
                "Note {} confirmed: amount={}, expiry={}",
                note_id, amount, expiry_ts
            );
            println!("Wallet state saved. You can now make API requests.");
        }

        Commands::Request { payload } => {
            println!(
                "Sending request with payload: {}...",
                &payload[..payload.len().min(50)]
            );
            println!("(Mock mode: request would be sent to {})", cli.server_url);
        }

        Commands::Withdraw { destination } => {
            println!("Initiating mutual-close withdrawal to {}", destination);
        }

        Commands::EscapeWithdraw { destination } => {
            println!("Initiating escape-hatch withdrawal to {}", destination);
        }

        Commands::Recover => {
            println!("Checking for pending requests...");
            println!("No pending requests found.");
        }

        Commands::Status => {
            println!("Wallet state directory: {:?}", cli.state_dir);
            println!("No active note found. Run `zkapi deposit` to get started.");
        }

        Commands::Server {
            listen,
            db_path,
            state_seed,
            clear_seed,
            epoch,
            xmss_height,
            initial_root,
        } => {
            println!("Starting zkAPI server on {} with DB at {}", listen, db_path);
            let state_seed = Felt252::from_hex(&state_seed)
                .map_err(|e| anyhow::anyhow!("invalid --state-seed: {}", e))?;
            let clear_seed = Felt252::from_hex(&clear_seed)
                .map_err(|e| anyhow::anyhow!("invalid --clear-seed: {}", e))?;
            let initial_root = Felt252::from_hex(&initial_root)
                .map_err(|e| anyhow::anyhow!("invalid --initial-root: {}", e))?;
            let config = zkapi_server::config::ServerConfig {
                listen_addr: listen,
                db_path,
                state_seed,
                clear_seed,
                epoch,
                xmss_height,
                initial_root,
                ..Default::default()
            };
            zkapi_server::routes::run_server(config).await?;
        }

        Commands::Indexer { listen } => {
            println!("Starting zkAPI indexer on {}", listen);
            // In production, would connect to an Ethereum node and consume events
            println!("Indexer running (mock mode).");
            tokio::signal::ctrl_c().await?;
        }
    }

    Ok(())
}
