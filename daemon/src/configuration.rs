use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use optional_struct::optional_struct;
use serde::{Deserialize, Serialize};

/// Defines how the backup should be executed
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize, Deserialize)]
pub enum OperationalMode {
    /// Execute full backups
    #[serde(alias = "full")]
    Full,
    /// Execute incremental backups
    #[serde(alias = "incremental")]
    Incremental,
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
    #[arg(short, long, required_unless_present_all(&["operational_mode", "providers", "provider_list"]))]
    pub config:           Option<PathBuf>,
    /// Defines how the backup should be executed
    #[arg(short, long, value_enum, required_unless_present = "config")]
    pub operational_mode: Option<OperationalMode>,
    /// Whether to backup each folder in a separate branch for independent versioning and simplified
    /// management.
    #[arg(short, long)]
    pub folder_branching: bool,
    /// Whether to automatically split files that exceed a predefined size to optimize storage and
    /// ensure compatibility with repository limits.
    #[arg(short, long)]
    pub split:            bool,
    /// The size at which files should be split, if the split option is enabled.
    #[arg(long, default_value = "25", required_if_eq("split", "true"))]
    pub split_size:       u16,
    /// The unit of the split size.
    #[arg(
        long,
        default_value = "megabytes",
        required_if_eq("split", "true"),
        value_enum
    )]
    pub split_unit:       SizeUnit,
    /// Whether to record each file independently within the repository to facilitate granular
    /// backup and recovery processes. This will increase the overall backup size.
    #[arg(short, long)]
    pub multipart:        bool,
    /// The paths to backup. Each path can be a file or folder, in the case of a folder all its
    /// contents will be backed up recursively.
    #[arg(short, long)]
    pub paths:            Vec<String>,
    /// Whether to enable debug logging.
    #[arg(short, long)]
    pub debug:            bool,
    /// Whether to compress the backup before uploading it to the repository.
    #[arg(short, long)]
    pub compress:         bool,
    /// Whether to encrypt the backup before uploading it to the repository.
    #[arg(short, long)]
    pub encrypt:          bool,
    /// The encryption key to use, if encryption is enabled.
    #[arg(long, required_if_eq("encrypt", "true"))]
    pub key:              Option<String>,
    /// The storage provider to use for the backup.
    /// Use the gitup provider url format:
    /// `gitup://<auth-token>:<provider-name>/<provider-dependant-fragments>` For a full list of
    /// supported providers and their configuration options, run `gitup --provider-list`.
    #[arg(long, required_unless_present = "config")]
    pub providers:        Vec<String>,
    /// Executes Gitup in provider list mode, displaying a list of all available storage providers
    /// and their configuration options.
    #[arg(long, required_unless_present_any = &["config", "providers", "operational_mode"])]
    pub provider_list:    bool,
}
