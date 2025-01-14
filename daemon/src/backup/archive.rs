use std::{collections::HashMap, env, io::Write, path::PathBuf, sync::Arc, time::SystemTime};

use cuid2::create_id;
use futures::stream::{self, StreamExt};
use tokio::{
    fs::File,
    sync::{RwLock, Semaphore},
};
use tracing::{debug, error, info};
use zip::ZipWriter;

use crate::{
    backup::{
        compression::ensure_no_errors_counting_size,
        errors::BackupError,
        file::{make_readable_file, open_file_or_fail, read_buf, FileMode},
        structures::{BackupFile, BackupMetadata, FileBackupMetadata, FolderBackupMetadata},
        utility::{compute_size_variation, replace_recursive},
    },
    byte_size::format_bytesize,
    configuration::{DEFAULT_BUFFER_SIZE, MAX_THREADS},
};

pub async fn join_files(
    files: HashMap<PathBuf, Vec<BackupFile>>,
    metadata: Arc<RwLock<BackupMetadata>>,
) -> Result<Vec<String>, BackupError> {
    use tokio::io::AsyncReadExt;

    let now = SystemTime::now();
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    let tasks = stream::iter(files.into_iter())
        .map(|(folder, files)| {
            let semaphore = semaphore.clone();
            let metadata = metadata.clone();

            async move {
                let _permit = semaphore.acquire().await;
                process_folder(folder, files, metadata).await
            }
        })
        .buffer_unordered(*MAX_THREADS);

    let result: Vec<Result<String, BackupError>> = tasks.collect().await;

    let (archived_size, archive_paths) = ensure_no_errors_counting_size(result)?;

    let original_size = {
        let mut metadata = metadata.write().await;
        metadata.stats.archival_size = archived_size;
        metadata.stats.original_size - metadata.stats.duplicates_size
    };

    let duration = now.elapsed().unwrap_or_default();

    info!(
        "Created {} archive(s) in {:.2} seconds ({}, {})",
        archive_paths.len(),
        duration.as_secs_f64(),
        format_bytesize(archived_size),
        compute_size_variation(original_size as f64, archived_size as f64)
    );

    Ok(archive_paths)
}

/// Process a folder and create a zip archive
///
/// # Arguments
///
/// * `folder` - The folder to process
/// * `files` - The files to add to the zip archive
/// * `metadata` - The backup metadata
///
/// # Returns
///
/// The path to the zip archive
async fn process_folder(
    folder: PathBuf,
    files: Vec<BackupFile>,
    metadata: Arc<RwLock<BackupMetadata>>,
) -> Result<String, BackupError> {
    let folder_id = create_id();
    let zip_filename = format!("{}/{}.gitup", env::temp_dir().display(), folder_id);
    let file = open_file_or_fail(&zip_filename, FileMode::Create)
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;
    let mut zip = ZipWriter::new(file.into_std().await);

    let mut folder_meta = FolderBackupMetadata {
        original_path: folder.to_str().unwrap().to_string(),
        size:          0,
        id:            folder_id.clone(),
        files:         HashMap::new(),
    };

    for file in files {
        let mut filename = file.path.to_string_lossy().to_string();
        filename = replace_recursive(filename, "..", "");
        filename = replace_recursive(filename, ":", "");
        filename = replace_recursive(filename, "//", "/");
        filename = replace_recursive(filename, "/./", "/");
        filename = replace_recursive(filename, "?", "");
        filename = replace_recursive(filename, "\\", "/");

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);

        zip.start_file(filename.clone(), options)
            .map_err(|e| BackupError::CannotAddZipEntry(e.to_string()))?;

        let mut readable_file = make_readable_file(file.path.to_string_lossy().as_ref()).await?;
        let mut total_bytes_read = 0;

        // Read the file in chunks and write it to the zip archive
        loop {
            let fragment = read_buf(&mut readable_file, DEFAULT_BUFFER_SIZE, total_bytes_read).await;

            // Check if the fragment is an error, if it is, break the loop
            if fragment.is_err() {
                break;
            }
            let fragment = fragment.unwrap();

            // Update the total bytes read
            total_bytes_read = fragment.total_bytes_read;

            // Write the fragment to the zip file
            zip.write_all(fragment.data.as_slice())
                .map_err(|e| BackupError::CannotWriteToZipArchive(e.to_string()))?;
        }

        // Add the file to the folder metadata
        let file_id = create_id();
        folder_meta.size += file.size;
        folder_meta.files.insert(
            file_id.clone(),
            FileBackupMetadata {
                id:            file_id,
                original_path: filename,
                size:          file.size,
                last_updated:  file.last_updated,
                hash:          file.hash.clone(),
                duplicates:    file.duplicates,
            },
        );
    }

    // Finalize the zip archive
    zip.finish()
        .map_err(|e| BackupError::CannotFinalizeZipArchive(e.to_string()))?;

    let file_ids: Vec<String> = folder_meta.files.keys().cloned().collect();
    let mut metadata = metadata.write().await;
    metadata.tree.insert(folder_id.clone(), folder_meta);

    for file_id in file_ids.iter() {
        metadata
            .reverse_lookup
            .insert(file_id.clone(), folder_id.clone());
    }

    Ok(zip_filename)
}
