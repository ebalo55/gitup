use std::{fmt::Display, fs, path::PathBuf, thread};

use clap::{Parser, ValueEnum};
use once_cell::sync::Lazy;
use optional_struct::{optional_struct, Applicable};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::byte_size::MEGABYTE;

/// The size of the encryption buffer
pub static DEFAULT_BUFFER_SIZE: u64 = 1 * MEGABYTE;
/// The size of the decryption buffer
pub static DECRYPTION_BUFFER_SIZE: u64 = DEFAULT_BUFFER_SIZE + 24; // 192 bits per nonce
/// The name of the metadata file
pub static SNAPSHOT_FILE: &str = "gitup.snap";
pub static MAX_THREADS: Lazy<usize> = Lazy::new(|| {
    // This must fail if the number of CPUs cannot be determined
    let cpus = thread::available_parallelism().unwrap();
    debug!("Number of CPUs: {}", cpus);

    // The number of threads is the number of CPUs
    cpus.get()
});

/// Defines how the backup should be executed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize, Default)]
pub enum OperationalMode {
    /// Execute full backups
    #[serde(alias = "full")]
    #[default]
    Full,
    /// Execute incremental backups
    #[serde(alias = "incremental")]
    Incremental,
    /// Restore a backup
    #[serde(alias = "restore")]
    Restore,
}

impl Display for OperationalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationalMode::Full => write!(f, "full"),
            OperationalMode::Incremental => write!(f, "incremental"),
            OperationalMode::Restore => write!(f, "restore"),
        }
    }
}

/// Data size unit
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
pub enum SizeUnit {
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
pub struct Args {
    /// Load the configuration from a file. Note: Command line arguments will have a higher
    /// priority.
    #[arg(short, long, required_unless_present_any(&["operational_mode", "providers", "provider_list"]
    ))]
    pub config:           Option<PathBuf>,
    /// Defines how the backup should be executed
    #[arg(short, long, value_enum, required_unless_present_any(&["config", "query_snapshot", "provider_list"]
    ))]
    pub operational_mode: Option<OperationalMode>,
    /// Whether to backup each folder separately to facilitate granular backup and recovery.
    /// This will increase the overall backup size but drastically improve the recovery process
    /// speed in case of partial data restore.
    #[arg(short, long)]
    pub folder_branching: bool,
    /// The size at which files should be split, if the split option is enabled.
    #[arg(long, default_value = "25")]
    pub split_size:       u16,
    /// The unit of the split size.
    #[arg(long, default_value = "megabytes", value_enum)]
    pub split_unit:       SizeUnit,
    /// The paths to backup. Each path can be a file or folder, in the case of a folder all its
    /// contents will be backed up recursively.
    #[arg(short, long)]
    pub paths:            Vec<String>,
    /// Whether to enable debug logging.
    #[arg(short, long)]
    pub debug:            bool,
    /// Whether to compress the backup before uploading it to the repository.
    #[arg(long)]
    pub compress:         bool,
    /// Whether to encrypt the backup before uploading it to the repository.
    #[arg(short, long)]
    pub encrypt:          bool,
    /// The encryption key to use, if encryption is enabled.
    #[arg(long, required_if_eq("encrypt", "true"))]
    pub key:              Option<String>,
    /// The storage provider to use for the backup.
    /// Use the gitup provider url format:
    /// `gitup://<auth-token>:<provider-name>/<provider-dependant-fragments>`.
    /// For a full list of supported providers and their configuration options, run
    /// `gitup --provider-list`.
    #[arg(long, required_unless_present = "config")]
    pub providers:        Vec<String>,
    /// Executes Gitup in provider list mode, displaying a list of all available storage providers
    /// and their configuration options.
    #[arg(long, required_unless_present_any = &["config", "providers", "operational_mode"])]
    pub provider_list:    bool,
    /// Whether to run the backup in dry-run mode, where no data is actually uploaded to the
    /// repositories
    #[arg(long)]
    pub dry_run:          bool,
    /// Whether to query the snapshot file of the backup to print the snapshot information
    #[arg(long)]
    pub query_snapshot:   Option<String>,
    /// Whether to display the snapshot information in a rich query format
    #[arg(long)]
    pub rich_query:       bool,
    /// Whether to print logs in JSON format
    #[arg(long)]
    pub json:             bool,
    /// The incremental backup to run the backup against, if any.
    /// This is the snapshot file of the previous backup to run the incremental backup against. It
    /// is used to determine the changes between the previous backup and the current one.
    #[arg(long)]
    pub incremental_on:   Option<String>,
    /// The snapshot file to restore from, if mode is set to restore.
    #[arg(long, required_if_eq("operational_mode", "restore"))]
    pub restore_from:     Option<String>,
}

/// Merges the configuration file with the command line arguments
pub fn merge_configuration_file(mut args: Args) -> Result<Args, String> {
    if args.config.is_some() && args.config.as_ref().unwrap().is_file() {
        let config_ref = args.config.as_ref().unwrap();
        debug!("Configuration file found, loading ...");
        info!("Loading configuration from '{}'", config_ref.display());

        // Load configuration from file
        let config = fs::read_to_string(config_ref.as_path());

        if config.is_err() {
            error!(
                "Failed to read configuration file: {}",
                config.as_ref().err().unwrap()
            );
            return Err(config.err().unwrap().to_string());
        }

        debug!("Parsing configuration ...");
        let config = serde_json::from_str::<OptionalArgs>(config.unwrap().as_str());
        if config.is_err() {
            error!(
                "Failed to parse configuration file: {}",
                config.as_ref().err().unwrap()
            );
            return Err(config.err().unwrap().to_string());
        }
        let config = config.unwrap();

        // override some boolean flags from configuration if they are false in the command line (aka not
        // set)
        if config.debug.is_some() && !args.debug {
            args.debug = config.debug.unwrap();
        }
        if config.compress.is_some() && !args.compress {
            args.compress = config.compress.unwrap();
        }
        if config.encrypt.is_some() && !args.encrypt {
            args.encrypt = config.encrypt.unwrap();
        }
        if config.dry_run.is_some() && !args.dry_run {
            args.dry_run = config.dry_run.unwrap();
        }
        if config.folder_branching.is_some() && !args.folder_branching {
            args.folder_branching = config.folder_branching.unwrap();
        }
        if config.json.is_some() && !args.json {
            args.json = config.json.unwrap();
        }

        debug!("Merging configuration ...");
        // Merge configuration from file with command line arguments
        let config = config.build(args);

        info!("Configuration loaded successfully");

        return Ok(config);
    }

    info!("No configuration file found, using command line arguments");
    Ok(args)
}
