mod archive;
mod cleanup;
pub mod compression;
pub mod encrypt;
pub mod errors;
mod estimate;
pub mod file;
mod snapshot;
pub mod structures;
mod upload;
mod upload_checks;
mod utility;

use std::{
    collections::HashMap,
    env,
    fs,
    fs::Metadata,
    io::{BufReader, Write},
    mem,
    path::PathBuf,
    sync::Arc,
    time::SystemTime,
};

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore,
    Key,
    XChaCha20Poly1305,
};
use cuid2::create_id;
use flate2::{bufread::ZlibEncoder, Compression};
use futures::{future::join_all, stream, StreamExt};
use glob::glob;
use hkdf::Hkdf;
use optional_struct::optional_struct;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256, Sha3_512};
pub use snapshot::query_snapshot;
use tokio::{
    fs::{remove_file, File},
    io::{AsyncWriteExt, BufWriter},
    sync::{Mutex, RwLock, RwLockReadGuard, Semaphore},
};
use tracing::{debug, error, info, warn};
use zip::ZipWriter;

use crate::{
    backup::{
        archive::join_files,
        cleanup::cleanup_temp_folder,
        compression::{compress_archives, ensure_no_errors_counting_size},
        encrypt::encrypt_archives,
        errors::BackupError,
        estimate::estimate_snapshot_size,
        snapshot::store_snapshot,
        structures::{BackupFile, BackupMetadata, BackupPart, BackupStats, FileBackupMetadata, FolderBackupMetadata},
        upload::upload_backup,
        upload_checks::check_available_size,
        utility::compute_size_variation,
    },
    byte_size::{format_bytesize, GIGABYTE, KILOBYTE, MEGABYTE},
    configuration::{Args, SizeUnit, DEFAULT_BUFFER_SIZE, SNAPSHOT_FILE},
    padding::pad_number,
    storage_providers::provider::StorageProvider,
};

/// Executes the backup process
///
/// # Arguments
///
/// * `args` - The command line arguments
/// * `providers` - The list of storage providers
///
/// # Returns
///
/// An empty result
pub async fn backup(args: &Args, providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>) -> Result<(), BackupError> {
    let now = SystemTime::now();

    let metadata = Arc::new(RwLock::new(BackupMetadata::default()));

    {
        let mut metadata = metadata.write().await;
        metadata.mode = args.operational_mode.unwrap();

        if let Some(incremental_on) = &args.incremental_on {
            metadata.previous_backup = Some(incremental_on.clone());
        }
    }

    // TODO: handle incremental backups, filtering out non-changed files

    let files = map_files(&args.paths)?;
    let backup_files = dedupe_files(files, metadata.clone()).await;

    // Create the metadata
    let backup_files = backup_files.1;

    // Group the files by folder if the option is enabled
    let grouped_files = if args.folder_branching {
        group_files_by_folder(backup_files)
    }
    else {
        let mut grouped_files = HashMap::new();
        grouped_files.insert(PathBuf::new(), backup_files);
        grouped_files
    };

    info!("Backing up ...");

    if args.dry_run {
        warn!("Dry-run mode enabled, no data will be uploaded to the storage providers");
    }

    let archives = join_files(grouped_files, metadata.clone()).await?;
    debug!(
        "{} archive(s) created at '{}'",
        archives.len(),
        env::temp_dir().display()
    );

    // compress the archives
    let archives = compress_archives(
        archives,
        metadata.clone(),
        if !args.compress {
            Compression::fast()
        }
        else {
            Compression::best()
        },
    )
    .await?;

    let mut archives = if args.encrypt && args.key.is_some() {
        // Encrypt the archives
        let encrypted_archives = encrypt_archives(args.key.as_ref().unwrap(), archives, metadata.clone()).await?;
        debug!(
            "{} archive(s) encrypted at '{}'",
            encrypted_archives.len(),
            env::temp_dir().display()
        );
        encrypted_archives
    }
    else {
        archives
    };

    // Check if there's enough space on the storage providers
    let final_size = {
        let metadata = metadata.read().await;

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

    info!("Checking available space on storage providers");

    let snapshot_size = estimate_snapshot_size(args, metadata.clone()).await as u64;
    let estimated_final_size = final_size + snapshot_size;
    debug!(
        "Snapshot estimated size: {}",
        format_bytesize(snapshot_size)
    );

    if !args.dry_run {
        check_available_size(providers.clone(), estimated_final_size).await?;

        // Upload the archives
        upload_backup(args, archives, metadata.clone(), providers.clone()).await?;
        cleanup_temp_folder();
    }
    else {
        warn!("Dry-run mode enabled, no data will be uploaded to the storage providers");
        debug!("Moving backup data to local folder");

        let mut new_archives = Vec::new();

        for archive in archives {
            let archive_path = PathBuf::from(archive.clone());
            let archive_name = archive_path.file_name().ok_or(BackupError::GeneralError(
                "Failed to get archive name".to_string(),
            ))?;
            let archive_name = archive_name.to_str().ok_or(BackupError::GeneralError(
                "Failed to convert archive name to string".to_string(),
            ))?;

            let new_path = PathBuf::from(format!("./{}", archive_name));
            new_archives.push(new_path.clone().display().to_string());

            fs::rename(archive, new_path.clone()).map_err(|e| BackupError::GeneralError(e.to_string()))?;
            debug!(
                "Archive '{}' moved to '{}'",
                archive_name,
                new_path.display()
            );
        }

        archives = new_archives;
    }

    let snapshot = store_snapshot(metadata.clone()).await?;
    debug!("Snapshot stored at '{}'", snapshot);

    let snapshot_size = file_size(&snapshot).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    let original_size = {
        let metadata = metadata.read().await;
        metadata.stats.original_size
    };
    let backup_size = final_size + snapshot_size;

    let duration = now.elapsed().unwrap_or_default();
    info!("Backup completed in {:.2} seconds", duration.as_secs_f64());
    info!("Original size: {}", format_bytesize(original_size));
    info!("Backup size: {}", format_bytesize(backup_size));
    info!(
        "Backup size variation: {}",
        compute_size_variation(original_size as f64, backup_size as f64)
    );

    Ok(())
}

/// Returns the size of a file
///
/// # Arguments
///
/// * `file_path` - The path to the file
///
/// # Returns
///
/// The size of the file in bytes
fn file_size(file_path: &str) -> Result<u64, std::io::Error> {
    let metadata = fs::metadata(file_path)?;
    Ok(metadata.len())
}

/// Groups the files by folder
///
/// # Arguments
///
/// * `files` - The list of files to group
///
/// # Returns
///
/// A hashmap containing the files grouped by folder
fn group_files_by_folder(files: Vec<BackupFile>) -> HashMap<PathBuf, Vec<BackupFile>> {
    let mut grouped_files = HashMap::new();

    for file in files {
        if !grouped_files.contains_key(&file.folder) {
            grouped_files.insert(file.folder.clone(), Vec::new());
        }

        grouped_files.get_mut(&file.folder).unwrap().push(file);
    }

    grouped_files
}

/// Maps the provided paths to a list of files
///
/// # Arguments
///
/// * `paths` - The list of paths to map
///
/// # Returns
///
/// A list of files
fn map_files(paths: &Vec<String>) -> Result<Vec<PathBuf>, BackupError> {
    let mut files = Vec::new();

    for path in paths {
        for entry in glob(&path).map_err(|_| BackupError::MalformedPattern(path.clone()))? {
            match entry {
                Ok(path) => {
                    if path.is_file() {
                        debug!("Adding file '{}' to backup list", path.display());
                        files.push(path);
                    }
                },
                Err(e) => {
                    error!("Failed to read path: {}", e);
                    warn!("Ignoring path '{}'", path);
                },
            }
        }
    }

    Ok(files)
}

/// Hashes a file using the SHA3-512 algorithm and returns the hash and the file metadata
///
/// # Arguments
///
/// * `file` - The file to hash
///
/// # Returns
///
/// A tuple containing the hash and the file metadata
async fn hash_file(file: &PathBuf) -> Result<(String, Metadata), BackupError> {
    use tokio::io::AsyncReadExt;

    // Open the file
    let file_to_hash = File::options().read(true).open(file).await;

    if file_to_hash.is_err() {
        return Err(BackupError::CannotReadFile(file.display().to_string()));
    }
    let file_to_hash = file_to_hash.unwrap();

    let metadata = file_to_hash
        .metadata()
        .await
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    // init the hasher and stream reader
    let mut hasher = Sha3_256::new();
    let mut reader = tokio::io::BufReader::new(file_to_hash);

    // Read the file and hash it in chunks of 16kb
    loop {
        let mut buffer = vec![0; 0x4000];
        let bytes_read = reader
            .read(&mut buffer)
            .await
            .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[.. bytes_read]);
    }
    let hash = format!("{:x}", hasher.finalize());

    Ok((hash, metadata))
}

/// Deduplicates a list of files by hashing them and comparing the hashes
///
/// # Arguments
///
/// * `files` - The list of files to deduplicate
///
/// # Returns
///
/// A list of deduplicated files
async fn dedupe_files(files: Vec<PathBuf>, metadata: Arc<RwLock<BackupMetadata>>) -> ((u64, u64), Vec<BackupFile>) {
    // create a hashmap to store the files and the hash as a lookup string
    let mut tmp_files: Arc<Mutex<HashMap<String, BackupFile>>> = Arc::new(Mutex::new(HashMap::new()));
    let mut pending_futures = Vec::new();

    // Shared duplicates counter
    let duplicates = Arc::new(Mutex::new(0));
    let duplicate_size = Arc::new(Mutex::new(0));
    let backup_size = Arc::new(Mutex::new(0));

    let files_len = files.len();
    let now = SystemTime::now();

    // Hash all files in parallel
    for file in files {
        let tmp_files = tmp_files.clone();
        // Clone the shared counters for each task
        let duplicates = duplicates.clone();
        let duplicate_size = duplicate_size.clone();
        let backup_size = backup_size.clone();

        pending_futures.push(tokio::spawn(async move {
            let hash = hash_file(&file).await;

            if hash.is_err() {
                error!(
                    "Failed to hash file '{}': {}",
                    file.display(),
                    hash.err().unwrap()
                );
                return;
            }
            let (hash, metadata) = hash.unwrap();

            let mut tmp_files = tmp_files.lock().await;

            // Check if the hash already exists in the hashmap, if so, add the file to the duplicates list
            if tmp_files.contains_key(&hash) {
                let mut tmp_file = tmp_files.get_mut(&hash).unwrap();
                tmp_file.duplicates.push(file.clone());

                debug!(
                    "File '{}' is a duplicate of '{}'",
                    file.display(),
                    tmp_file.path.display()
                );
                // explicitly drop the reference to avoid a deadlock
                drop(tmp_files);

                // Increment the duplicates counter
                let mut duplicates = duplicates.lock().await;
                *duplicates += 1;
                // explicitly drop the reference to avoid a deadlock
                drop(duplicates);

                // Increment the duplicate size counter
                let mut duplicate_size = duplicate_size.lock().await;
                *duplicate_size += metadata.len();
            }
            else {
                tmp_files.insert(
                    hash.clone(),
                    BackupFile {
                        hash,
                        folder: file.parent().unwrap().to_path_buf(),
                        path: file.clone(),
                        size: metadata.len(),
                        last_updated: metadata
                            .modified()
                            .unwrap_or(SystemTime::now())
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        duplicates: Vec::new(),
                    },
                );

                // Increment the backup size counter
                let mut backup_size = backup_size.lock().await;
                *backup_size += metadata.len();
            }
        }));
    }

    // Wait for all futures to complete
    join_all(pending_futures).await;
    let duration = now.elapsed().unwrap_or_default();

    // Retrieve the duplicates count
    let duplicates = { *duplicates.lock().await };
    let duplicate_size = { *duplicate_size.lock().await };
    let backup_size = { *backup_size.lock().await };

    // Update the metadata with the stats
    {
        let mut metadata = metadata.write().await;
        metadata.stats.duplicates = duplicates;
        metadata.stats.duplicates_size = duplicate_size;
        metadata.stats.files = files_len as u64 - duplicates;

        // The original size is the size of the backup before deduplication
        metadata.stats.original_size = backup_size;
    }

    let size_variation = (duplicate_size as f64 / backup_size as f64 * 100.0) * -1.;
    debug!("size_variation: {:?}", size_variation);

    debug!(
        "Hashed {} files in {} seconds",
        files_len,
        duration.as_secs_f64()
    );
    info!(
        "Found {} duplicates, backup size reduced by {} ({:.2}%)",
        duplicates,
        format_bytesize(duplicate_size),
        size_variation
    );
    info!(
        "Found {} unique files in {:.2} seconds ({})",
        files_len as u64 - duplicates,
        duration.as_secs_f64(),
        format_bytesize(backup_size)
    );
    let files = {
        let mut data = tmp_files.lock().await;
        mem::take(&mut *data)
    };
    (
        (backup_size, duplicate_size),
        files.into_iter().map(|(_, v)| v).collect(),
    )
}
