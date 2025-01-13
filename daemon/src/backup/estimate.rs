use std::sync::Arc;

use cuid2::create_id;
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::{
    backup::structures::{BackupMetadata, BackupStats, FileBackupMetadata, FolderBackupMetadata},
    byte_size::{GIGABYTE, KILOBYTE, MEGABYTE},
    configuration::{Args, SizeUnit},
};

/// Estimates the size of the backup metadata
///
/// # Arguments
///
/// * `args` - The command line arguments
/// * `metadata` - The metadata to estimate the size of
///
/// # Returns
///
/// The estimated size of the metadata
pub async fn estimate_snapshot_size(args: &Args, metadata: Arc<RwLock<BackupMetadata>>) -> usize {
    let metadata = metadata.read().await;

    let mut total_size = 0;

    // Fixed-size fields
    total_size += size_of::<u64>() * 3; // size_on_disk, deduped_size, snapped_at
    total_size += size_of::<bool>(); // encrypted

    // Optional encryption key
    if let Some(key) = &metadata.key {
        total_size += key.len();
    }

    // BackupStats size
    total_size += estimate_backup_stats_size(&metadata.stats);

    // Tree field (HashMap<String, FolderBackupMetadata>)
    for (folder_name, folder_metadata) in &metadata.tree {
        total_size += folder_name.len();
        total_size += estimate_folder_backup_metadata_size(folder_metadata);
    }

    // Reverse lookup table (HashMap<String, String>)
    for (key, value) in &metadata.reverse_lookup {
        total_size += key.len();
        total_size += value.len();
    }

    // precompute the size of the parts via inference
    total_size += estimate_parts_size(args, metadata);

    total_size
}

/// Get the size of the backup
///
/// # Arguments
///
/// * `metadata` - The metadata to get the size of
///
/// # Returns
///
/// The size of the backup
pub fn get_backup_size(metadata: &RwLockReadGuard<BackupMetadata>) -> u64 {
    if metadata.encrypted {
        metadata.stats.encrypted_size
    }
    else if metadata.stats.compressed_size != 0 {
        metadata.stats.compressed_size
    }
    else {
        metadata.stats.archival_size
    }
}

/// Estimates the size of the parts
///
/// # Arguments
///
/// * `args` - The command line arguments
/// * `metadata` - The metadata to estimate the size of
///
/// # Returns
///
/// The estimated size of the parts
fn estimate_parts_size(args: &Args, metadata: RwLockReadGuard<BackupMetadata>) -> usize {
    let mut total_size = 0;

    let size = match args.split_unit {
        SizeUnit::Kilobytes => args.split_size as u64 * KILOBYTE,
        SizeUnit::Megabytes => args.split_size as u64 * MEGABYTE,
        SizeUnit::Gigabytes => args.split_size as u64 * GIGABYTE,
    };

    let backup_size = get_backup_size(&metadata);
    let full_parts: u64 = backup_size / size;
    let remainder = backup_size % size;
    let mut total_parts = if remainder != 0 {
        full_parts + 1
    }
    else {
        full_parts
    };

    let sample_path_len = format!("{}.gitup.part", create_id()).len();

    for i in 0 .. total_parts {
        total_size += size_of::<u64>(); // size
        total_size += sample_path_len + i.to_string().len(); // path
        total_size += 64; // signature (256 bit = 32 bytes, in hex = 64 characters)
        total_size += "gitup://<auth-token>:<provider-name>/<provider-dependant-fragments>".len() * 3;
        // provider
    }

    total_size
}

/// Estimates the size of the BackupStats struct
///
/// # Arguments
///
/// * `stats` - The stats to estimate the size of
///
/// # Returns
///
/// The estimated size of the stats
fn estimate_backup_stats_size(stats: &BackupStats) -> usize {
    // All fields in BackupStats are fixed-size `u64`
    size_of::<u64>() * 9
}

/// Estimates the size of the FolderBackupMetadata struct
///
/// # Arguments
///
/// * `metadata` - The metadata to estimate the size of
///
/// # Returns
///
/// The estimated size of the metadata
fn estimate_folder_backup_metadata_size(metadata: &FolderBackupMetadata) -> usize {
    let mut total_size = 0;

    // Fixed-size fields
    total_size += size_of::<u64>(); // size

    // Strings
    total_size += metadata.id.len();
    total_size += metadata.original_path.len();

    // Files (HashMap<String, FileBackupMetadata>)
    for (file_name, file_metadata) in &metadata.files {
        total_size += file_name.len();
        total_size += estimate_file_backup_metadata_size(file_metadata);
    }

    total_size
}

/// Estimates the size of the FileBackupMetadata struct
///
/// # Arguments
///
/// * `metadata` - The metadata to estimate the size of
///
/// # Returns
///
/// The estimated size of the metadata
fn estimate_file_backup_metadata_size(metadata: &FileBackupMetadata) -> usize {
    let mut total_size = 0;

    // Fixed-size fields
    total_size += size_of::<u64>() * 2; // size, last_updated

    // Strings
    total_size += metadata.id.len();
    total_size += metadata.original_path.len();
    total_size += metadata.hash.len();

    total_size
}
