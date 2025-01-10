use std::{path::PathBuf, sync::Arc, time::SystemTime};

use flate2::Compression;
use serde::Serialize;
use tokio::{fs::File, io::AsyncWriteExt, sync::RwLock, time::sleep};
use tracing::{debug, error, info};

use crate::{
    backup::{
        compression::compress,
        errors::BackupError,
        file::{open_file_or_fail, FileMode},
        file_size,
        structures::BackupMetadata,
        utility::compute_size_variation,
    },
    byte_size::format_bytesize,
    configuration::SNAPSHOT_FILE,
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
pub async fn store_snapshot(metadata: Arc<RwLock<BackupMetadata>>) -> Result<(), BackupError> {
    let metadata = metadata.read().await;

    // Create the path for the snapshot file (<current_timestamp>-<SNAPSHOT_FILE>)
    let metadata_path = PathBuf::from(format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        SNAPSHOT_FILE
    ));

    // Open the file for writing
    let mut snapshot_file = open_file_or_fail(
        metadata_path.display().to_string().as_str(),
        FileMode::Create,
    )
    .await
    .map_err(|e| BackupError::CannotCreateBackupFile(e.to_string()))?;

    // Serialize the metadata
    let mut serialized_metadata = Vec::new();
    metadata
        .serialize(&mut rmp_serde::Serializer::new(&mut serialized_metadata))
        .map_err(|e| BackupError::GeneralError(format!("Failed to serialize snapshot: {}", e)))?;

    // Write the metadata to the file
    snapshot_file
        .write_all(serialized_metadata.as_slice())
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    sleep(std::time::Duration::from_secs(1)).await;

    // Get the size of the uncompressed snapshot
    let uncompressed_size = file_size(metadata_path.display().to_string().as_str())
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    // Compress the snapshot
    let compressed_snapshot = compress(
        metadata_path.display().to_string(),
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

    info!("Snapshot stored at '{}'", metadata_path.display());
    info!("Snapshots are not encrypted, make sure to store them securely as they are your source of restore");

    Ok(())
}
