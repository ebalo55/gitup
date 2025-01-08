mod errors;

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
use tokio::{
    fs::{remove_file, File},
    io::{AsyncWriteExt, BufWriter},
    sync::{Mutex, RwLock, Semaphore},
};
use tracing::{debug, error, info, warn};
use zip::ZipWriter;

use crate::configuration::METADATA_FILE;
use crate::{
    backup::errors::BackupError,
    byte_size::{format_bytesize, GIGABYTE, KILOBYTE, MEGABYTE},
    configuration::{Args, SizeUnit, ENCRYPTION_BUFFER_SIZE},
    storage_providers::provider::StorageProvider,
};

/// Represents the metadata of the backup
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct BackupMetadata {
    /// The size of the backup in bytes
    size_on_disk: u64,
    /// Size of the duplicates found in the backup, in bytes
    deduped_size: u64,
    /// The of folders and files in the backup with their metadata
    tree: HashMap<String, FolderBackupMetadata>,
    /// A reverse lookup table that allows to quickly find the folder of a file given the file hash
    reverse_lookup: HashMap<String, String>,
    /// The time the backup was snapped at in seconds since the Unix epoch
    snapped_at: u64,
    /// Whether the backup is encrypted
    encrypted: bool,
    /// The encryption key if any
    key: Option<String>,
    /// The stats of the backup
    stats: BackupStats,
    /// The parts of the backup
    parts: Vec<BackupPart>,
}

/// Represents a part of the backup, used when splitting the backup into multiple parts
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct BackupPart {
    /// The size of the part in bytes
    size: u64,
    /// The path to the part
    path: String,
    /// The signature of the part
    signature: String,
    /// Provider where the part is stored
    provider: String,
}

/// Represent the stats of the backup
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
struct BackupStats {
    /// The number of duplicates removed
    duplicates: u64,
    /// The size of the duplicates removed (in bytes)
    duplicates_size: u64,
    /// The number of files in the backup
    files: u64,
    /// The original size of the backup (in bytes).
    /// This actually is the size of the files once restored
    original_size: u64,
    /// The size of the backup (in bytes) after deduplication
    deduped_size: u64,
    /// The size of the backup (in bytes) after archiving
    archival_size: u64,
    /// The size of the backup (in bytes) after compression
    compressed_size: u64,
    /// The size of the backup (in bytes) after encryption
    encrypted_size: u64,
    /// The number of archives created (if splitting has been requested)
    archives: u64,
}

/// Represents the metadata of a folder in the backup
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
struct FolderBackupMetadata {
    /// The folder identifier
    #[serde(skip_serializing)]
    id: String,
    /// The original path of the folder
    original_path: String,
    /// The size of the folder in bytes
    size: u64,
    /// The list of files in the folder
    files: HashMap<String, FileBackupMetadata>,
}

/// Represents the metadata of a file in the backup
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
struct FileBackupMetadata {
    /// The file identifier
    #[serde(skip_serializing)]
    id: String,
    /// The original path of the file
    original_path: String,
    /// The size of the file in bytes
    size: u64,
    /// The last time the file was updated in seconds since the Unix epoch
    last_updated: u64,
    /// The hash of the file
    hash: String,
}

/// Represents a file to be backed up
#[optional_struct]
#[derive(Debug)]
struct BackupFile {
    /// The path to the folder where the file is located
    folder: PathBuf,
    /// The path to the file
    path: PathBuf,
    /// The size of the file in bytes
    size: u64,
    /// The last time the file was updated in seconds since the Unix epoch
    last_updated: u64,
    /// The hash of the file
    hash: String,
    /// The list of paths where the file have been found duplicated
    duplicates: Vec<PathBuf>,
}

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

    let files = map_files(&args.paths)?;
    let backup_files = dedupe_files(files, metadata.clone()).await;

    // Create the metadata
    {
        let mut metadata = metadata.write().await;
        metadata.size_on_disk = backup_files.0.0;
        metadata.deduped_size = backup_files.0.1;
    }
    let backup_files = backup_files.1;

    // Group the files by folder if the option is enabled
    let grouped_files = if args.folder_branching {
        group_files_by_folder(backup_files)
    } else {
        let mut grouped_files = HashMap::new();
        grouped_files.insert(PathBuf::new(), backup_files);
        grouped_files
    };

    info!("Backing up ...");
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
        if args.compress {
            Compression::fast()
        } else {
            Compression::best()
        },
    )
        .await?;

    let archives = if args.encrypt && args.key.is_some() {
        // Encrypt the archives
        let encrypted_archives = encrypt_archives(args.key.as_ref().unwrap(), archives, metadata.clone()).await?;
        debug!(
            "{} archive(s) encrypted at '{}'",
            encrypted_archives.len(),
            env::temp_dir().display()
        );
        encrypted_archives
    } else {
        archives
    };

    // Check if there's enough space on the storage providers
    let final_size = {
        let metadata = metadata.read().await;

        if metadata.stats.encrypted_size != 0 {
            metadata.stats.encrypted_size
        } else if metadata.stats.compressed_size != 0 {
            metadata.stats.compressed_size
        } else {
            metadata.stats.archival_size
        }
    };

    // Update the metadata with the final size
    {
        let mut metadata = metadata.write().await;
        metadata.size_on_disk = final_size;
    }

    info!("Checking available space on storage providers");

    let metadata_size = estimate_backup_metadata_size(args, metadata.clone()).await as u64;
    let final_size = final_size + metadata_size;
    debug!(
        "Metadata estimated size: {}",
        format_bytesize(metadata_size)
    );
    check_available_size(providers.clone(), final_size).await?;

    upload_backup(args, archives, metadata.clone(), providers.clone()).await?;

    cleanup_temp_folder();

    store_metadata(metadata.clone()).await?;

    let duration = now.elapsed().unwrap_or_default();
    info!(
        "Backup completed in {:.2} seconds",
        duration.as_secs_f64()
    );

    Ok(())
}

async fn store_metadata(metadata: Arc<RwLock<BackupMetadata>>) -> Result<(), BackupError> {
    let metadata = metadata.read().await;

    let metadata_path = PathBuf::from(METADATA_FILE);
    let metadata_file = File::create(&metadata_path).await;
    if metadata_file.is_err() {
        error!(
            "Failed to create metadata file '{}': {}",
            metadata_path.display(),
            metadata_file.as_ref().err().unwrap()
        );
        return Err(BackupError::CannotCreateBackupFile(
            metadata_file.err().unwrap().to_string(),
        ));
    }
    let mut metadata_file = metadata_file.unwrap();

    let mut serialized_metadata = Vec::new();
    metadata.serialize(&mut rmp_serde::Serializer::new(&mut serialized_metadata)).map_err(|e| {
        BackupError::GeneralError(format!("Failed to serialize metadata: {}", e))
    })?;

    metadata_file
        .write_all(serialized_metadata.as_slice())
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    info!("Metadata stored at '{}'", metadata_path.display());
    info!("Metadata are not encrypted, make sure to store them securely as they are your source of restore");

    Ok(())
}

fn pad_number(num: u32, length: usize) -> String { format!("{:0width$}", num, width = length) }

/// Uploads the backup to the storage providers
///
/// # Arguments
///
/// * `args` - The command line arguments
/// * `archives` - The archives to upload
/// * `metadata` - The metadata of the backup
/// * `providers` - The list of storage providers
///
/// # Returns
///
/// An empty result
async fn upload_backup(
    args: &Args,
    archives: Vec<String>,
    metadata: Arc<RwLock<BackupMetadata>>,
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
) -> Result<(), BackupError> {
    use tokio::io::AsyncReadExt;
    let now = SystemTime::now();

    // Calculate the size of the parts
    let split_size = match args.split_unit {
        SizeUnit::Kilobytes => args.split_size as u64 * KILOBYTE,
        SizeUnit::Megabytes => args.split_size as u64 * MEGABYTE,
        SizeUnit::Gigabytes => args.split_size as u64 * GIGABYTE,
    };
    // Create the rule to apply when splitting the backup
    let split_rule = args.split.clone();

    if split_rule {
        debug!(
            "Splitting the backup into parts of {}",
            format_bytesize(split_size)
        );
    } else {
        warn!("Splitting the backup into parts disabled, the backup will be stored as a single file(s)");
    }

    // Limit the number of concurrent tasks
    let max_concurrent_uploads = 4; // Adjust this as needed
    let semaphore = Arc::new(Semaphore::new(max_concurrent_uploads));

    let tasks = stream::iter(archives.into_iter())
        .map(|archive| {
            let semaphore = semaphore.clone();
            let metadata = metadata.clone();
            let providers = providers.clone();
            let split_rule = split_rule.clone();

            async move {
                // Acquire the semaphore
                let _permit = semaphore.acquire().await;
                debug!("Uploading archive '{}'", archive);

                let file = File::options().read(true).open(&archive).await;

                if file.is_err() {
                    error!(
                        "Failed to open archive '{}': {}",
                        archive,
                        file.as_ref().err().unwrap()
                    );
                    return Err(BackupError::CannotReadFile(file.err().unwrap().to_string()));
                }
                let file = file.unwrap();
                let file_meta = file
                    .metadata()
                    .await
                    .map_err(|e| BackupError::GeneralError(e.to_string()))?;

                let mut reader = tokio::io::BufReader::new(file);

                // read the provider, one call only to avoid multiple locks
                let mut providers = providers.read().await;

                if split_rule {
                    // Calculate the number of parts to derive the padding length
                    let mut max_parts = file_meta.len() / split_size;
                    if file_meta.len() % split_size != 0 {
                        max_parts += 1;
                    }
                    let padding_length = max_parts.to_string().len();

                    // Split the file into parts, calculate the hash of each part and upload it
                    let mut index = 1;
                    let mut total_bytes_read = 0;
                    let file_length = file_meta.len();

                    loop {
                        // Ensure we don't read beyond the file's total size
                        if total_bytes_read >= file_length {
                            break;
                        }

                        // Calculate the remaining bytes to read
                        let remaining_bytes = (file_length - total_bytes_read) as usize;
                        let buffer_size = remaining_bytes.min(split_size as usize);

                        // create the buffer to store the part
                        let mut buffer = vec![0; buffer_size];

                        // read the part
                        let bytes_read = reader
                            .read_exact(&mut buffer)
                            .await
                            .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

                        if bytes_read == 0 {
                            // EOF reached
                            break;
                        }

                        buffer.resize(bytes_read, 0);

                        total_bytes_read += bytes_read as u64;

                        // Calculate the hash of the part
                        let mut hasher = Sha3_256::new();
                        hasher.update(&buffer);
                        let hash = hasher.finalize();

                        // Find a provider with enough space to store the part
                        let mut provider_index = 0;
                        let mut provider = providers.first().unwrap();
                        let mut upload_failed = true;

                        // create a reference to the buffer to be shared across multiple loops without copying it
                        let buffer = Arc::new(buffer);

                        while upload_failed && provider_index < providers.len() {
                            let buffer = buffer.clone();

                            // Check if the provider has enough space, if not, move to the next one
                            let available_space = provider.get_available_space();
                            if available_space < buffer_size as u64 {
                                warn!(
                                    "Provider '{}' does not have enough space to store the archive '{}' (available: {}, required: {}), moving to the next provider",
                                    format_bytesize(available_space),
                                    format_bytesize(buffer_size as u64),
                                    provider.name(),
                                    archive
                                );
                                provider_index += 1;

                                // Try to get the next provider or return an error if there are no more providers
                                if let Some(next_provider) = providers.get(provider_index) {
                                    provider = next_provider;
                                } else {
                                    error!(
                                        "No provider has enough space to store the archive '{}'",
                                        archive
                                    );
                                    return Err(BackupError::NotEnoughSpace);
                                }

                                continue;
                            }

                            // Create the part metadata
                            let part = BackupPart {
                                size: bytes_read as u64,
                                path: format!(
                                    "{}.gitup.part{}",
                                    create_id(),
                                    pad_number(index, padding_length)
                                ),
                                signature: format!("{:x}", hash),
                                provider: provider.to_string(),
                            };

                            // Upload the part
                            let response = provider.upload(part.path.clone(), buffer.clone()).await;
                            if response.is_err() {
                                error!(
                                    "Failed to upload part '{}' to provider '{}': {}",
                                    part.path,
                                    provider.name(),
                                    response.as_ref().err().unwrap()
                                );
                                warn!(
                                    "Trying to upload part '{}' to the next provider",
                                    part.path
                                );
                                provider_index += 1;

                                // Try to get the next provider or return an error if there are no more providers
                                if let Some(next_provider) = providers.get(provider_index) {
                                    provider = next_provider;
                                } else {
                                    error!(
                                        "All providers failed to store the archive '{}'",
                                        archive
                                    );
                                    return Err(BackupError::NotEnoughSpace);
                                }

                                continue;
                            }

                            debug!(
                                "Part '{}' uploaded to provider '{}'",
                                part.path,
                                provider.name()
                            );

                            // Update the metadata with the part
                            let mut metadata = metadata.write().await;
                            metadata.parts.push(part);
                            drop(metadata);

                            upload_failed = false;
                        }

                        index += 1;
                    }
                } else {
                    let mut buffer = Vec::new();
                    // Read the whole file in memory this is STRONGLY DISCOURAGED as it can lead to memory
                    // exhaustion very quickly as we're running in multiple threads
                    reader
                        .read_to_end(&mut buffer)
                        .await
                        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

                    // Calculate the hash of the file
                    let mut hasher = Sha3_256::new();
                    hasher.update(&buffer);
                    let hash = hasher.finalize();

                    // Find a provider with enough space to store the file
                    let mut provider_index = 0;
                    let mut provider = providers.first().unwrap();
                    let mut upload_failed = true;

                    // create a reference to the buffer to be shared across multiple loops without copying it
                    let buffer = Arc::new(buffer);

                    while upload_failed && provider_index < providers.len() {
                        let buffer = buffer.clone();

                        // Check if the provider has enough space, if not, move to the next one
                        let available_space = provider.get_available_space();
                        let file_length = file_meta.len() as u64;
                        if available_space < file_length {
                            warn!(
                                "Provider '{}' does not have enough space to store the archive '{}' (available: {}, required: {}), moving to the next provider",
                                format_bytesize(available_space),
                                format_bytesize(file_length),
                                provider.name(),
                                archive
                            );
                            provider_index += 1;

                            // Try to get the next provider or return an error if there are no more providers
                            if let Some(next_provider) = providers.get(provider_index) {
                                provider = next_provider;
                            } else {
                                error!(
                                    "No provider has enough space to store the archive '{}'",
                                    archive
                                );
                                return Err(BackupError::NotEnoughSpace);
                            }

                            continue;
                        }

                        // Create the part metadata
                        let part = BackupPart {
                            size: file_meta.len(),
                            path: format!(
                                "{}.gitup",
                                create_id(),
                            ),
                            signature: format!("{:x}", hash),
                            provider: provider.to_string(),
                        };

                        // Upload the part
                        let response = provider.upload(part.path.clone(), buffer).await;

                        if response.is_err() {
                            error!(
                                "Failed to upload part '{}' to provider '{}': {}",
                                part.path,
                                provider.name(),
                                response.as_ref().err().unwrap()
                            );
                            warn!(
                                "Trying to upload part '{}' to the next provider",
                                part.path
                            );
                            provider_index += 1;

                            // Try to get the next provider or return an error if there are no more providers
                            if let Some(next_provider) = providers.get(provider_index) {
                                provider = next_provider;
                            } else {
                                error!(
                                    "All providers failed to store the archive '{}'",
                                    archive
                                );
                                return Err(BackupError::NotEnoughSpace);
                            }

                            continue;
                        }

                        debug!(
                            "Part '{}' uploaded to provider '{}'",
                            part.path,
                            provider.name()
                        );

                        // Update the metadata with the part
                        let mut metadata = metadata.write().await;
                        metadata.parts.push(part);
                        drop(metadata);

                        upload_failed = false;
                    }
                }

                Ok(())
            }
        })
        .buffer_unordered(max_concurrent_uploads);

    let result: Vec<Result<(), BackupError>> = tasks.collect().await;

    for res in result {
        if res.is_err() {
            return Err(res.err().unwrap());
        }
    }

    let duration = now.elapsed().unwrap_or_default();

    info!(
        "Backup uploaded in {:.2} seconds",
        duration.as_secs_f64()
    );

    Ok(())
}

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
async fn estimate_backup_metadata_size(args: &Args, metadata: Arc<RwLock<BackupMetadata>>) -> usize {
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
    if args.split {
        let size = match args.split_unit {
            SizeUnit::Kilobytes => args.split_size as u64 * KILOBYTE,
            SizeUnit::Megabytes => args.split_size as u64 * MEGABYTE,
            SizeUnit::Gigabytes => args.split_size as u64 * GIGABYTE,
        };

        let full_parts: u64 = metadata.size_on_disk / size;
        let remainder = metadata.size_on_disk % size;
        let mut total_parts = if remainder != 0 {
            full_parts + 1
        } else {
            full_parts
        };

        let sample_path_len = PathBuf::from(format!("{}.gitup.part", create_id()))
            .display()
            .to_string()
            .len();

        for i in 0..total_parts {
            total_size += size_of::<u64>(); // size
            total_size += sample_path_len + i.to_string().len(); // path
            total_size += 128; // signature (512 bit = 64 bytes, in hex = 128 characters)
            total_size += "gitup://<auth-token>:<provider-name>/<provider-dependant-fragments>".len() * 3;
            // provider
        }
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

/// Check if there is enough space available on the storage providers
///
/// # Arguments
///
/// * `providers` - The list of storage providers
/// * `size` - The size to check for
///
/// # Returns
///
/// An empty result if there is enough space, an error otherwise
async fn check_available_size(
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
    size: u64,
) -> Result<(), BackupError> {
    let providers = providers.read().await;

    let mut available_size = 0;
    for provider in providers.iter() {
        available_size += provider.get_available_space();
    }

    if available_size < size {
        return Err(BackupError::NotEnoughSpace);
    }

    debug!(
        "Available space: {}, Required space: {}",
        format_bytesize(available_size),
        format_bytesize(size)
    );
    info!("Enough space available on storage providers, uploading ...");

    Ok(())
}

/// Cleans up the temporary folder
///
/// This function will remove all the files in the temporary folder that have been created during
/// the backup process
fn cleanup_temp_folder() {
    debug!("Cleaning up temporary folder");

    for entry in glob(&format!("{}/*.gitup*", env::temp_dir().display())).unwrap() {
        match entry {
            Ok(path) => {
                if path.is_file() {
                    let _ = fs::remove_file(path);
                }
            }
            Err(e) => {
                error!("Failed to read path: {}", e);
            }
        }
    }
}

/// Compresses the archives
///
/// # Arguments
///
/// * `archives` - The archives to compress
/// * `compression` - The compression algorithm to use
///
/// # Returns
///
/// A list of paths to the compressed archives
async fn compress_archives(
    archives: Vec<String>,
    metadata: Arc<RwLock<BackupMetadata>>,
    compression: Compression,
) -> Result<Vec<String>, BackupError> {
    use std::io::Read;

    let now = SystemTime::now();

    let compression = Arc::new(compression);

    let mut pending_futures = Vec::new();
    for archive in archives {
        let compression = compression.clone();
        pending_futures.push(tokio::spawn(async move {
            // Open the archive
            let file = File::options().read(true).open(&archive).await;
            if file.is_err() {
                error!(
                    "Failed to open archive '{}': {}",
                    archive,
                    file.as_ref().err().unwrap()
                );
                return Err(BackupError::CannotReadFile(file.err().unwrap().to_string()));
            }
            let file = file.unwrap();

            // Create the compressed archive
            let compressed_filename = format!("{}z", archive);
            let compressed_file = File::create(compressed_filename.clone()).await;
            if compressed_file.is_err() {
                error!(
                    "Failed to create compressed archive '{}': {}",
                    compressed_filename,
                    compressed_file.as_ref().err().unwrap()
                );
                return Err(BackupError::CannotCreateBackupFile(
                    compressed_file.err().unwrap().to_string(),
                ));
            }
            let compressed_file = compressed_file.unwrap();
            let mut writer = BufWriter::new(compressed_file);

            // prepare the compressor, reader and writer
            let mut compressor = ZlibEncoder::new(BufReader::new(file.into_std().await), *compression);

            // read the file and write it to the compressed file in chunks of ENCRYPTION_BUFFER_SIZE
            loop {
                let mut buffer = vec![0; ENCRYPTION_BUFFER_SIZE as usize];
                let bytes_read = compressor
                    .read(&mut buffer)
                    .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

                if bytes_read == 0 {
                    break;
                }

                writer
                    .write(&buffer[..bytes_read])
                    .await
                    .map_err(|e| BackupError::CannotWriteToZipArchive(e.to_string()))?;
            }

            // remove the original archive
            remove_file(archive)
                .await
                .map_err(|e| BackupError::GeneralError(e.to_string()))?;

            Ok(compressed_filename)
        }));
    }

    // Wait for all futures to complete
    let result = join_all(pending_futures).await;

    let mut compressed_size = 0;
    let mut compressed_archives = Vec::new();

    // Check if any of the futures failed
    for res in result {
        if res.is_err() {
            return Err(BackupError::GeneralError(res.err().unwrap().to_string()));
        }
        let res = res.unwrap()?;
        compressed_archives.push(res.clone());
        compressed_size += file_size(res.as_str()).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;
    }

    // Update the metadata with the stats
    let original_size = {
        let mut metadata = metadata.write().await;
        metadata.stats.compressed_size = compressed_size;
        metadata.stats.archival_size
    };

    let duration = now.elapsed().unwrap_or_default();
    // Calculate the size variation
    let size_variation = ((original_size as f64 - compressed_size as f64) / original_size as f64) * 100.0 * -1.;
    debug!("size_variation: {:?}", size_variation);

    info!(
        "Compressed {} archive(s) in {:.2} seconds ({}, {}{:.2}%)",
        compressed_archives.len(),
        duration.as_secs_f64(),
        format_bytesize(compressed_size),
        if size_variation >= 0. { "+" } else { "" },
        size_variation
    );

    Ok(compressed_archives)
}

/// Encrypts the archives using the provided key
///
/// # Arguments
///
/// * `key` - The encryption key
/// * `archives` - The archives to encrypt
/// * `metadata` - The metadata of the backup
///
/// # Returns
///
/// A list of paths to the encrypted archives
async fn encrypt_archives(
    key: &String,
    archives: Vec<String>,
    metadata: Arc<RwLock<BackupMetadata>>,
) -> Result<Vec<String>, BackupError> {
    use tokio::io::AsyncReadExt;

    let now = SystemTime::now();

    // Generate the encryption key and nonce
    let hkdf_salt = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let hkdf = Hkdf::<Sha3_512>::new(Some(hkdf_salt.as_slice()), key.as_bytes());
    let mut key = [0u8; 32];
    hkdf.expand(b"gitup", &mut key)
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    // Hash the key and store it in the metadata
    let mut hasher = Sha3_512::new();
    hasher.update(key);
    hasher.update(hkdf_salt.as_slice());
    let hash = format!("{:x}", hasher.finalize());
    let salt = format!("{:x}", hkdf_salt);

    let mut working_metadata = metadata.write().await;
    working_metadata.encrypted = true;
    working_metadata.key = Some(format!("{}{}", hash, salt));
    drop(working_metadata);

    // make the key sharable
    let key = Arc::new(key);

    let mut pending_futures = Vec::new();
    for archive in archives {
        let key = key.clone();
        pending_futures.push(tokio::spawn(async move {
            // Open the archive
            let file = File::options().read(true).open(&archive).await;
            if file.is_err() {
                error!(
                    "Failed to open archive '{}': {}",
                    archive,
                    file.as_ref().err().unwrap()
                );
                return Err(BackupError::CannotReadFile(file.err().unwrap().to_string()));
            }
            let file = file.unwrap();

            // Create the encrypted archive
            let encrypted_filename = format!("{}c", archive);
            let encrypted_file = File::create(encrypted_filename.clone()).await;
            if encrypted_file.is_err() {
                error!(
                    "Failed to create encrypted archive '{}': {}",
                    encrypted_filename,
                    encrypted_file.as_ref().err().unwrap()
                );
                return Err(BackupError::CannotCreateBackupFile(
                    encrypted_file.err().unwrap().to_string(),
                ));
            }
            let encrypted_file = encrypted_file.unwrap();

            // prepare cipher, reader and writer
            let mut cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
            let mut reader = tokio::io::BufReader::new(file);
            let mut writer = tokio::io::BufWriter::new(encrypted_file);

            // read the file and write it to the encrypted file in chunks of ENCRYPTION_BUFFER_SIZE
            loop {
                let mut buffer = vec![0; ENCRYPTION_BUFFER_SIZE as usize];
                let bytes_read = reader
                    .read(&mut buffer)
                    .await
                    .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

                // if the end of the file is reached, break the loop
                if bytes_read == 0 {
                    break;
                }

                // generate a nonce and encrypt the buffer
                let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                let ciphertext = cipher
                    .encrypt(&nonce, &buffer[..bytes_read])
                    .map_err(|e| BackupError::CannotEncrypt(e.to_string()))?;

                // write the nonce and the ciphertext to the encrypted file
                writer
                    .write(&nonce)
                    .await
                    .map_err(|e| BackupError::CannotEncrypt(e.to_string()))?;
                writer
                    .write(&ciphertext)
                    .await
                    .map_err(|e| BackupError::CannotEncrypt(e.to_string()))?;
            }

            // remove the original archive
            remove_file(archive)
                .await
                .map_err(|e| BackupError::GeneralError(e.to_string()))?;

            Ok(encrypted_filename)
        }));
    }

    // Wait for all futures to complete
    let result = join_all(pending_futures).await;

    let mut encrypted_size = 0;
    let mut encrypted_archives = Vec::new();

    // Check if any of the futures failed
    for res in result {
        if res.is_err() {
            return Err(BackupError::GeneralError(res.err().unwrap().to_string()));
        }
        let res = res.unwrap()?;
        encrypted_archives.push(res.clone());
        encrypted_size += file_size(res.as_str()).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;
    }

    // Update the metadata with the stats
    let original_size = {
        let mut working_metadata = metadata.write().await;
        working_metadata.stats.encrypted_size = encrypted_size;

        // return the size relative to the previous step
        if working_metadata.stats.compressed_size == 0 {
            working_metadata.stats.archival_size
        } else {
            working_metadata.stats.compressed_size
        }
    };

    let duration = now.elapsed().unwrap_or_default();
    // Calculate the size variation
    let size_variation = ((original_size as f64 - encrypted_size as f64) / original_size as f64) * 100.0 * -1.;
    debug!("size_variation: {:?}", size_variation);

    info!(
        "Encrypted {} archive(s) in {:.2} seconds ({}, {}{:.2}%)",
        encrypted_archives.len(),
        duration.as_secs_f64(),
        format_bytesize(encrypted_size),
        if size_variation >= 0. { "+" } else { "" },
        size_variation
    );

    Ok(encrypted_archives)
}

/// Joins the files into one or more zip archives
///
/// # Arguments
///
/// * `files` - The files to join
/// * `metadata` - The metadata of the backup
///
/// # Returns
///
/// A list of paths to the created zip archives
async fn join_files(
    files: HashMap<PathBuf, Vec<BackupFile>>,
    metadata: Arc<RwLock<BackupMetadata>>,
) -> Result<Vec<String>, BackupError> {
    use tokio::io::AsyncReadExt;

    let now = SystemTime::now();

    let mut pending_futures = Vec::new();

    for (folder, files) in files {
        let metadata = metadata.clone();

        pending_futures.push(tokio::spawn(async move {
            let folder_id = create_id();

            // Create the backup file
            let zip_filename = format!("{}/{}.gitup", env::temp_dir().display(), folder_id);
            let file = File::create(zip_filename.clone()).await;
            if file.is_err() {
                error!(
                    "Failed to create backup file: {}",
                    file.as_ref().err().unwrap()
                );
                return Err(BackupError::CannotCreateBackupFile(
                    file.err().unwrap().to_string(),
                ));
            }
            let file = file.unwrap();

            // Make it a zip archive
            let mut zip = ZipWriter::new(file.into_std().await);

            // Initialize the folder metadata
            let mut folder_meta = FolderBackupMetadata {
                original_path: folder.to_str().unwrap().to_string(),
                size: 0,
                id: folder_id.clone(),
                files: HashMap::new(),
            };

            // Add the files to the zip archive
            for file in files {
                // get the filename from the path
                let filename = file
                    .path
                    .canonicalize()
                    .map_err(|e| BackupError::GeneralError(e.to_string()))?
                    .display()
                    .to_string()
                    .replace(":", "")
                    .replace("//", "/")
                    .replace("\\", "/")
                    .replace("?", "")
                    .replace("///", "");

                // prepare the file options
                let mut options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o755);
                if file.size >= u32::MAX as u64 - 0x10000u64 {
                    options = options.large_file(true);
                }

                // debug!("Adding file '{}' to zip archive", file.path.display());
                // debug!("File {:?}", file);

                // add the file to the zip archive
                zip.start_file(filename, options)
                   .map_err(|e| BackupError::CannotAddZipEntry(e.to_string()))?;

                // open the file in read mode
                let reader = File::options().read(true).open(&file.path).await;
                if reader.is_err() {
                    error!(
                        "Failed to open file '{}': {}",
                        file.path.display(),
                        reader.as_ref().err().unwrap()
                    );
                    return Err(BackupError::CannotReadFile(
                        reader.err().unwrap().to_string(),
                    ));
                }
                let reader = reader.unwrap();
                let mut reader = tokio::io::BufReader::new(reader);

                // read the file and write it to the zip archive in chunks of 16kb
                loop {
                    let mut buffer = vec![0; 0x4000];
                    let bytes_read = reader
                        .read(&mut buffer)
                        .await
                        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

                    if bytes_read == 0 {
                        break;
                    }

                    zip.write_all(&buffer[..bytes_read])
                       .map_err(|e| BackupError::CannotWriteToZipArchive(e.to_string()))?;
                }

                // update the folder metadata
                let file_id = create_id();
                folder_meta.size += file.size;
                folder_meta.files.insert(
                    file_id.clone(),
                    FileBackupMetadata {
                        id: file_id,
                        original_path: file.path.to_str().unwrap().to_string(),
                        size: file.size,
                        last_updated: file.last_updated,
                        hash: file.hash.clone(),
                    },
                );

                // update the metadata for the duplicate files
                // NOTE: the duplicates, wherever they are, are not added to the zip archive nor to their real
                // folder but instead are added to the metadata of the FIRST occurrence of the file.
                // Example:
                // - `file1.txt` is found in `folder1`, `folder2` and `folder3`
                // - `file1.txt` is added to the zip archive of `folder1`
                // - `folder1` will contain the metadata of `file1.txt` and the partial metadata (with a different
                //   id as they are different files) of `file1.txt` in `folder2` and `folder3`
                // - if `folder2` (or `folder3`) is backed up, the metadata of `file1.txt` will NOT be present as
                //   already included in the backup of `folder1`
                for duplicate in file.duplicates {
                    let file_id = create_id();
                    folder_meta.files.insert(
                        file_id.clone(),
                        FileBackupMetadata {
                            id: file_id,
                            original_path: duplicate.to_str().unwrap().to_string(),
                            size: 0,
                            last_updated: file.last_updated,
                            hash: file.hash.clone(),
                        },
                    );
                }
            }

            // finalize the zip archive
            zip.finish()
               .map_err(|e| BackupError::CannotFinalizeZipArchive(e.to_string()))?;

            // clone the file ids to create the reverse lookup table
            let file_ids: Vec<String> = folder_meta.files.keys().cloned().collect();

            // update the metadata
            let mut metadata = metadata.write().await;

            // update the tree
            metadata.tree.insert(folder_id.clone(), folder_meta);

            // update the reverse lookup
            // insert the file ids in the reverse lookup table
            for file_id in file_ids.iter() {
                metadata
                    .reverse_lookup
                    .insert(file_id.clone(), folder_id.clone());
            }

            Ok(zip_filename)
        }));
    }

    // Wait for all futures to complete
    let result = join_all(pending_futures).await;

    let mut archived_size = 0;
    let mut archive_paths = Vec::new();
    // Check if any of the futures failed
    for res in result {
        if res.is_err() {
            return Err(BackupError::GeneralError(res.err().unwrap().to_string()));
        }
        let res = res.unwrap()?;
        archive_paths.push(res.clone());
        archived_size += file_size(res.as_str()).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;
    }

    // Update the metadata with the stats
    let original_size = {
        let mut metadata = metadata.write().await;
        metadata.stats.archival_size = archived_size;

        // return the size relative to the previous step
        metadata.stats.original_size - metadata.stats.deduped_size
    };

    let duration = now.elapsed().unwrap_or_default();
    // Calculate the size variation
    let size_variation = ((original_size as f64 - archived_size as f64) / original_size as f64) * 100.0 * -1.;
    debug!("size_variation: {:?}", size_variation);

    info!(
        "Created {} archive(s) in {:.2} seconds ({}, {}{:.2}%)",
        archive_paths.len(),
        duration.as_secs_f64(),
        format_bytesize(archived_size),
        if size_variation >= 0. { "+" } else { "" },
        size_variation
    );

    Ok(archive_paths)
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
                }
                Err(e) => {
                    error!("Failed to read path: {}", e);
                    warn!("Ignoring path '{}'", path);
                }
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

        hasher.update(&buffer[..bytes_read]);
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
            } else {
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
