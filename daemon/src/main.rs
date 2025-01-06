use std::fs;
use std::path::PathBuf;
use clap::{Parser, ValueEnum};
use optional_struct::{optional_struct, Applicable};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, Level};
use tracing::field::debug;
use tracing_subscriber::layer::SubscriberExt;

/// Defines how the backup should be executed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
enum OperationalMode {
    /// Execute full backups
    Full,
    /// Execute incremental backups
    Incremental,
}

/// Data size unit
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
enum SizeUnit {
    /// Represent a chunk of 1024 bytes
    Kilobytes,
    /// Represent a chunk of 1024 kilobytes
    Megabytes,
    /// Represent a chunk of 1024 megabytes
    Gigabytes,
}

/// Gitup daemon process, handles the hard work in the background
#[optional_struct]
#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
struct Args {
    /// Load the configuration from a file. Note: Command line arguments will have an higher priority.
    #[arg(short, long, required_unless_present = "operational_mode")]
    config: PathBuf,
    /// Defines how the backup should be executed
    #[arg(short, long, value_enum, required_unless_present = "config")]
    operational_mode: OperationalMode,
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
    pat: String,
    /// Github or Gitlab repository URL (no, ssh is not supported)
    #[arg(short, long, required_unless_present = "config")]
    repository_url: String,
}

#[tokio::main]
async fn main() {
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

    if args.config.is_file() {
        debug!("Configuration file found, loading ...");
        info!("Loading configuration from '{}'", args.config.display());

        // Load configuration from file
        let config = fs::read_to_string(args.config.as_ref());

        if config.is_err() {
            tracing::error!("Failed to read configuration file: {}", config.err().unwrap());
            return;
        }

        debug!("Parsing configuration ...");
        let config = serde_json::from_str::<OptionalArgs>(config.unwrap().as_str());
        if config.is_err() {
            tracing::error!("Failed to parse configuration file: {}", config.err().unwrap());
            return;
        }
        let config = config.unwrap();

        debug!("Merging configuration ...");
        // Merge configuration from file with command line arguments
        config.build(args);

        info!("Configuration loaded successfully");
    }

    println!("{:?}", args);
}
