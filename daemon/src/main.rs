mod web_client;

use crate::web_client::{make_request, RequestEndpoint, UsefulMetadata};
use clap::{Parser, ValueEnum};
use optional_struct::{optional_struct, Applicable};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::field::debug;
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::layer::SubscriberExt;

/// Defines how the backup should be executed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
enum OperationalMode {
    /// Execute full backups
    #[serde(alias = "full")]
    Full,
    /// Execute incremental backups
    #[serde(alias = "incremental")]
    Incremental,
}

/// Data size unit
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
enum SizeUnit {
    /// Represent a chunk of 1024 bytes
    #[serde(alias = "kilobytes")]
    Kilobytes,
    /// Represent a chunk of 1024 kilobytes
    #[serde(alias = "megabytes")]
    Megabytes,
    /// Represent a chunk of 1024 megabytes
    #[serde(alias = "gigabytes")]
    Gigabytes,
}

/// Gitup daemon process, handles the hard work in the background
#[optional_struct]
#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
struct Args {
    /// Load the configuration from a file. Note: Command line arguments will have an higher priority.
    #[arg(short, long, required_unless_present_all(&["operational_mode", "pat", "repository_url"]))]
    config: Option<PathBuf>,
    /// Defines how the backup should be executed
    #[arg(short, long, value_enum, required_unless_present = "config")]
    operational_mode: Option<OperationalMode>,
    /// Whether to backup each folder in a separate Git branch for independent versioning and simplified
    /// management.
    #[arg(short, long)]
    folder_branching: bool,
    /// Whether to automatically split files that exceed a predefined size to optimize storage and ensure
    /// compatibility with repository limits.
    #[arg(short, long)]
    split: bool,
    /// The size at which files should be split, if the split option is enabled.
    #[arg(long, default_value = "25", required_if_eq("split", "true"))]
    split_size: u16,
    /// The unit of the split size.
    #[arg(
        long,
        default_value = "megabytes",
        required_if_eq("split", "true"),
        value_enum
    )]
    split_unit: SizeUnit,
    /// Whether to record each file independently within the repository to facilitate granular backup and recovery
    /// processes.
    #[arg(short, long)]
    multipart: bool,
    /// The paths to backup. Each path can be a file or folder, in the case of a folder all its contents will be
    /// backed up recursively.
    #[arg(short, long)]
    paths: Vec<String>,
    /// Whether to enable debug logging.
    #[arg(short, long)]
    debug: bool,
    /// Github or Gitlab personal access token
    #[arg(long, required_unless_present = "config")]
    pat: Option<String>,
    /// Github or Gitlab repository URL (no, ssh is not supported)
    #[arg(short, long, required_unless_present = "config")]
    repository_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = Args::parse();

    // Setup tracing
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_ansi(true)
            .with_level(true)
            .with_file(false)
            .with_line_number(false)
            .compact()
            .finish()
            .with(tracing_subscriber::filter::filter_fn(
                move |metadata| match metadata.level() {
                    &Level::TRACE => false,
                    &Level::DEBUG => args.debug,
                    _ => true,
                },
            )),
    )
    .unwrap();

    let config = if args.config.is_some() && args.config.as_ref().unwrap().is_file() {
        let config_ref = args.config.as_ref().unwrap();
        debug!("Configuration file found, loading ...");
        info!("Loading configuration from '{}'", config_ref.display());

        // Load configuration from file
        let config = fs::read_to_string(config_ref.as_path());

        if config.is_err() {
            tracing::error!(
                "Failed to read configuration file: {}",
                config.err().unwrap()
            );
            return;
        }

        debug!("Parsing configuration ...");
        let config = serde_json::from_str::<OptionalArgs>(config.unwrap().as_str());
        if config.is_err() {
            tracing::error!(
                "Failed to parse configuration file: {}",
                config.err().unwrap()
            );
            return;
        }
        let config = config.unwrap();

        debug!("Merging configuration ...");
        // Merge configuration from file with command line arguments
        let config = config.build(args);

        info!("Configuration loaded successfully");

        config
    } else {
        info!("No configuration file found, using command line arguments");

        args
    };

    info!("Starting Gitup daemon ...");

    debug!("Checking connection to repository ...");
    check_connection(&config).await?;
    debug!("Connection to repository successful");

    Ok(())
}

/// Check the connection to the repository
async fn check_connection(args: &Args) -> Result<(), String> {
    let repository_url = args.repository_url.as_ref().unwrap();
    let pat = args.pat.as_ref().unwrap();

    // prepare the request
    let req = make_request(RequestEndpoint::Meta, repository_url, pat);
    if req.is_err() {
        error!("Failed to create request: {}", req.as_ref().err().unwrap());
        return Err(req.err().unwrap().to_string());
    }
    let req = req.unwrap();
    let response = req.send().await;


    if response.is_err() {
        error!(
            "Failed to connect to repository: {}",
            response.as_ref().err().unwrap()
        );
        return Err(response.err().unwrap().to_string());
    }
    let response = response.unwrap();

    if response.status().is_success() {
        let response = response.json::<UsefulMetadata>().await;

        if response.is_err() {
            error!(
                "Failed to parse response: {}",
                response.as_ref().err().unwrap()
            );
            return Err(response.err().unwrap().to_string());
        }
        let response = response.unwrap();

        if response.archived {
            error!("Repository archived, cannot continue");
            return Err("Repository archived, cannot continue".to_string());
        }
        if response.disabled {
            error!("Repository disabled, cannot continue");
            return Err("Repository disabled, cannot continue".to_string());
        }

        debug!("Repository visibility: {}", response.visibility);

        if response.visibility != "private" && response.visibility != "internal" {
            warn!("Repository is public, consider making it private");
        }

        return Ok(());
    }

    error!("Failed to connect to repository: {}", response.status());
    Err(format!(
        "Request responded with status {}",
        response.status().to_string()
    ));
}
