use std::{collections::HashMap, path::PathBuf};

use optional_struct::optional_struct;
use serde::{Deserialize, Serialize};

use crate::configuration::OperationalMode;

/// Represents the metadata of the backup
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackupMetadata {
    /// The of folders and files in the backup with their metadata
    pub tree:            HashMap<String, FolderBackupMetadata>,
    /// A reverse lookup table that allows to quickly find the folder of a file given the file hash
    pub reverse_lookup:  HashMap<String, String>,
    /// The time the backup was snapped at in seconds since the Unix epoch
    pub snapped_at:      u64,
    /// Whether the backup is encrypted
    pub encrypted:       bool,
    /// The encryption key if any
    pub key:             Option<String>,
    /// The stats of the backup
    pub stats:           BackupStats,
    /// The parts of the backup
    pub parts:           Vec<BackupPart>,
    /// The operational mode of the backup
    pub mode:            OperationalMode,
    /// The previous backup in case of an incremental backup
    pub previous_backup: Option<String>,
}

/// Represents a part of the backup, used when splitting the backup into multiple parts
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BackupPart {
    /// The size of the part in bytes
    pub size:      u64,
    /// The path to the part
    pub path:      String,
    /// The signature of the part
    pub signature: String,
    /// Provider where the part is stored
    pub provider:  String,
}

/// Represent the stats of the backup
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct BackupStats {
    /// The number of duplicates removed
    pub duplicates:      u64,
    /// The size of the duplicates removed (in bytes)
    pub duplicates_size: u64,
    /// The number of files in the backup
    pub files:           u64,
    /// The original size of the backup (in bytes).
    /// This actually is the size of the files once restored
    pub original_size:   u64,
    /// The size of the backup (in bytes) after archiving
    pub archival_size:   u64,
    /// The size of the backup (in bytes) after compression
    pub compressed_size: u64,
    /// The size of the backup (in bytes) after encryption
    pub encrypted_size:  u64,
    /// The number of archives created (if splitting has been requested)
    pub archives:        u64,
}

/// Represents the metadata of a folder in the backup
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct FolderBackupMetadata {
    /// The folder identifier
    pub id:            String,
    /// The original path of the folder
    pub original_path: String,
    /// The size of the folder in bytes
    pub size:          u64,
    /// The list of files in the folder
    pub files:         HashMap<String, FileBackupMetadata>,
}

/// Represents the metadata of a file in the backup
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct FileBackupMetadata {
    /// The file identifier
    pub id:            String,
    /// The original path of the file
    pub original_path: String,
    /// The size of the file in bytes
    pub size:          u64,
    /// The last time the file was updated in seconds since the Unix epoch
    pub last_updated:  u64,
    /// The hash of the file
    pub hash:          String,
    /// The list of paths where the file have been found duplicated
    pub duplicates:    Vec<PathBuf>,
}

/// Represents a file to be backed up
#[optional_struct]
#[derive(Debug)]
pub struct BackupFile {
    /// The path to the folder where the file is located
    pub folder:       PathBuf,
    /// The path to the file
    pub path:         PathBuf,
    /// The size of the file in bytes
    pub size:         u64,
    /// The last time the file was updated in seconds since the Unix epoch
    pub last_updated: u64,
    /// The hash of the file
    pub hash:         String,
    /// The list of paths where the file have been found duplicated
    pub duplicates:   Vec<PathBuf>,
}
