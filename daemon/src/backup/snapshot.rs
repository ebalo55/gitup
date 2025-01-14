use std::{fs, mem, ops::Deref, path::PathBuf, sync::Arc, time::SystemTime};

use chrono::{DateTime, Local};
use colored::Colorize;
use flate2::Compression;
use serde::Serialize;
use tokio::{
    fs::File,
    io::AsyncWriteExt,
    sync::{RwLock, RwLockWriteGuard},
    time::sleep,
};
use tracing::{debug, error, info};

use crate::{
    backup::{
        compression::{compress, uncompress},
        errors::BackupError,
        file::{make_readable_file, open_file_or_fail, read_buf, FileMode},
        file_size,
        structures::BackupMetadata,
        utility::compute_size_variation,
    },
    byte_size::format_bytesize,
    configuration::{Args, DEFAULT_BUFFER_SIZE, SNAPSHOT_FILE},
};

/// Stores the snapshot of the backup
///
/// # Arguments
///
/// * `metadata` - The metadata of the backup
///
/// # Returns
///
/// An empty result
pub async fn store_snapshot(metadata: Arc<RwLock<BackupMetadata>>) -> Result<String, BackupError> {
    let mut metadata = metadata.write().await;

    let snapped_at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    metadata.snapped_at = snapped_at;

    // Create the path for the snapshot file (<current_timestamp>-<SNAPSHOT_FILE>)
    let snapshot_path = PathBuf::from(format!("{}-{}", snapped_at, SNAPSHOT_FILE));

    // Open the file for writing
    let mut snapshot_file = open_file_or_fail(
        snapshot_path.display().to_string().as_str(),
        FileMode::Create,
    )
    .await
    .map_err(|e| BackupError::CannotCreateBackupFile(e.to_string()))?;

    optimize_metadata(&mut metadata);

    // Serialize the metadata
    let serialized_metadata = serde_json::to_vec(metadata.deref())
        .map_err(|e| BackupError::GeneralError(format!("Failed to serialize snapshot: {}", e)))?;

    // Write the metadata to the file
    snapshot_file
        .write_all(&serialized_metadata)
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    sleep(std::time::Duration::from_secs(5)).await;

    // Get the size of the uncompressed snapshot
    let uncompressed_size = file_size(snapshot_path.display().to_string().as_str())
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    // Compress the snapshot
    let compressed_snapshot = compress(
        snapshot_path.display().to_string(),
        Arc::new(Compression::best()),
    )
    .await?;

    // Get the size of the compressed snapshot
    let compressed_size =
        file_size(compressed_snapshot.as_str()).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    debug!(
        "Compressed snapshot from {} to {} ({})",
        format_bytesize(uncompressed_size),
        format_bytesize(compressed_size),
        compute_size_variation(uncompressed_size as f64, compressed_size as f64)
    );

    info!("Snapshot stored at '{}'", compressed_snapshot);
    info!("Snapshots are not encrypted, make sure to store them securely as they are your source of restore");

    Ok(compressed_snapshot)
}

/// Optimizes the metadata of the backup
///
/// # Arguments
///
/// * `metadata` - The metadata to optimize
///
/// # Returns
///
/// An empty result
fn optimize_metadata(metadata: &mut RwLockWriteGuard<BackupMetadata>) {
    // Collect the values into a temporary vector
    let values: Vec<_> = metadata.reverse_lookup.values().cloned().collect();

    // optimize reverse lookup, if all files point to the same folder, we can mark the reverse lookup
    // with a special syntax ("*" = "<folder>")
    let mut folder = None;
    let mut all_same = true;
    for value in values {
        if folder.is_none() {
            folder = Some(value);
        }
        else if folder.as_ref().unwrap() != &value {
            all_same = false;
            break;
        }
    }

    if all_same {
        metadata.reverse_lookup.clear();
        metadata
            .reverse_lookup
            .insert("*".to_string(), folder.unwrap().clone());
    }
}

/// Queries the snapshot file of the backup to print the snapshot information
///
/// # Arguments
///
/// * `path` - The path to the snapshot file
///
/// # Returns
///
/// An empty result
pub async fn query_snapshot(path: String, print: bool, rich_text: bool) -> Result<BackupMetadata, BackupError> {
    use colored::Colorize;

    let snapshot = open_file_or_fail(path.as_str(), FileMode::Read)
        .await
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    // Get the size of the snapshot
    let snapshot_size = snapshot
        .metadata()
        .await
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?
        .len();

    debug!("Uncompressing snapshot ...");
    let uncompressed_snapshot_path = uncompress(path, false).await?;
    debug!("Snapshot uncompressed at '{}'", uncompressed_snapshot_path);

    let uncompressed_snapshot_size =
        file_size(uncompressed_snapshot_path.as_str()).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    debug!("Reading snapshot ...");
    let mut readable_snapshot = make_readable_file(uncompressed_snapshot_path.as_str()).await?;

    let mut snapshot_data = Vec::new();
    let mut total_bytes_read = 0;
    loop {
        let fragment = read_buf(
            &mut readable_snapshot,
            DEFAULT_BUFFER_SIZE,
            total_bytes_read,
        )
        .await;

        // Check if the fragment is an error, if it is, break the loop
        if fragment.is_err() {
            break;
        }
        let fragment = fragment.unwrap();

        // Update the total bytes read
        total_bytes_read = fragment.total_bytes_read;

        // Append the fragment to the snapshot data
        snapshot_data.extend_from_slice(fragment.data.as_slice());
    }
    debug!("Snapshot read into memory");

    debug!("Deserializing snapshot ...");
    let metadata: BackupMetadata = serde_json::from_slice(&snapshot_data)
        .map_err(|e| BackupError::GeneralError(format!("Failed to deserialize snapshot: {}", e)))?;
    debug!("Snapshot deserialized");

    if !print {
        return Ok(metadata);
    }

    let snap_datetime = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(metadata.snapped_at);
    let date_time: DateTime<Local> = DateTime::from(snap_datetime);
    let elapsed = Local::now() - date_time;
    let mut elapsed_string = String::new();
    if elapsed.num_days() > 0 {
        elapsed_string.push_str(&format!("{} days ", elapsed.num_days()));
    }
    if elapsed.num_hours() > 0 {
        elapsed_string.push_str(&format!("{} hours ", elapsed.num_hours()));
    }
    if elapsed.num_minutes() > 0 {
        elapsed_string.push_str(&format!("{} minutes ", elapsed.num_minutes()));
    }

    let final_size = {
        if metadata.stats.encrypted_size != 0 {
            metadata.stats.encrypted_size
        }
        else if metadata.stats.compressed_size != 0 {
            metadata.stats.compressed_size
        }
        else {
            metadata.stats.archival_size
        }
    };

    println!("{}", "Snapshot information".bold());

    if !rich_text {
        println!(
            "  {:<20} {:>20}",
            "Snapshot size".bold(),
            format_bytesize(snapshot_size)
        );

        println!(
            "  {:<20} {:>20}",
            "Uncompressed size".bold(),
            format_bytesize(uncompressed_snapshot_size)
        );

        println!(
            "  {:<20} {:>20}",
            "Mode".bold(),
            format!("{} backup", metadata.mode)
        );
        println!(
            "  {:<20} {:>20}",
            "Previous backup".bold(),
            metadata
                .previous_backup
                .as_ref()
                .unwrap_or(&"None".to_string())
        );
        println!(
            "  {:<20} {:>20}",
            "Created on".bold(),
            date_time.format("%Y-%m-%d %H:%M:%S")
        );

        println!(
            "  {:<20} {:>20}",
            "Time since snap".bold(),
            elapsed_string.trim_end()
        );

        println!("  {:<20} {:>20}", "Encrypted".bold(), metadata.encrypted);

        println!(
            "  {:<20} {:>20}",
            "Files".bold(),
            metadata.stats.files + metadata.stats.duplicates
        );

        println!(
            "  {:<20} {:>20}",
            "Duplicated files".bold(),
            metadata.stats.duplicates
        );

        println!(
            "  {:<20} {:>20}",
            "Original size".bold(),
            format_bytesize(metadata.stats.original_size)
        );

        println!(
            "  {:<20} {:>20}",
            "Backup size".bold(),
            format_bytesize(final_size)
        );

        println!(
            "  {:<20} {:>20}",
            "Size reduction".bold(),
            format_bytesize(metadata.stats.original_size - final_size)
        );

        println!(
            "  {:<20} {:>20}",
            "Size variation".bold(),
            compute_size_variation(metadata.stats.original_size as f64, final_size as f64)
        );

        println!(
            "  {:<20} {:>20}",
            "Parts".bold(),
            if metadata.parts.len() == 0 {
                1
            }
            else {
                metadata.parts.len()
            }
        );
        println!();

        for (key, value) in metadata.parts.iter().enumerate() {
            println!("  {}", format!("Part {}", key + 1).bold());
            println!("    {:<10} {:<20}", "Provider".bold(), value.provider);
            println!("    {:<10} {:<20}", "Path".bold(), value.path);
            println!(
                "    {:<10} {:<20}",
                "Size".bold(),
                format_bytesize(value.size)
            );
        }
    }
    else {
        println!(
            "  The snapshot operation was performed in '{}' mode",
            format!("{} backup", metadata.mode).italic()
        );

        if metadata.previous_backup.is_some() {
            println!(
                "  The snapshot references '{}' as its previous step",
                metadata.previous_backup.as_ref().unwrap().italic()
            );
        }

        println!(
            "  The snapshot was created on (yyyy-mm-dd) {}, at {}, about {}ago",
            date_time.format("%Y-%m-%d"),
            date_time.format("%H:%M:%S"),
            elapsed_string
        );

        if metadata.encrypted {
            println!("  The snapshot is encrypted");
        }
        else {
            println!("  The snapshot {} encrypted", "is not".underline());
        }

        println!(
            "  The snapshot contains {} files, {} of them are duplicated",
            metadata.stats.files + metadata.stats.duplicates,
            metadata.stats.duplicates
        );

        println!(
            "  The files were originally {}, after the backup the size was reduced to {} with a reduction of {} \
             saving about {}",
            format_bytesize(metadata.stats.original_size),
            format_bytesize(final_size),
            compute_size_variation(metadata.stats.original_size as f64, final_size as f64),
            format_bytesize(metadata.stats.original_size - final_size)
        );

        if metadata.parts.is_empty() {
            println!("  The snapshot is not split into parts");
        }
        else {
            println!(
                "  The snapshot is split into {} parts",
                metadata.parts.len()
            );
        }
    }

    // fs::remove_file(uncompressed_snapshot_path).map_err(|e|
    // BackupError::GeneralError(e.to_string()))?;

    Ok(metadata)
}
