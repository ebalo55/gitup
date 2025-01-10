use std::{future::Future, pin::Pin, slice::SliceIndex, sync::Arc, time::SystemTime};

use cuid2::create_id;
use futures::{stream, StreamExt};
use sha3::{Digest, Sha3_256};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::{RwLock, Semaphore},
};
use tracing::{debug, error, info, warn};

use crate::{
    backup::{
        errors::BackupError,
        file::{make_readable_file, read_buf},
        structures::{BackupMetadata, BackupPart},
    },
    byte_size::{format_bytesize, GIGABYTE, KILOBYTE, MEGABYTE},
    configuration::{Args, SizeUnit, MAX_THREADS},
    hash::sha3,
    padding::pad_number,
    storage_providers::provider::{ProviderError, StorageProvider},
};

/// Computes the size of the split
///
/// # Arguments
///
/// * `args` - The command line arguments
///
/// # Returns
///
/// The size of the split
fn compute_split_size(args: &Args) -> u64 {
    match args.split_unit {
        SizeUnit::Kilobytes => args.split_size as u64 * KILOBYTE,
        SizeUnit::Megabytes => args.split_size as u64 * MEGABYTE,
        SizeUnit::Gigabytes => args.split_size as u64 * GIGABYTE,
    }
}

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
pub async fn upload_backup(
    args: &Args,
    archives: Vec<String>,
    metadata: Arc<RwLock<BackupMetadata>>,
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
) -> Result<(), BackupError> {
    use tokio::io::AsyncReadExt;
    let now = SystemTime::now();

    // Calculate the size of the parts
    let split_size = compute_split_size(args);
    debug!(
        "Splitting the backup into parts of {}",
        format_bytesize(split_size)
    );

    // Limit the number of concurrent tasks
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    let tasks = stream::iter(archives.into_iter())
        .map(|archive| {
            let semaphore = semaphore.clone();
            let metadata = metadata.clone();
            let providers = providers.clone();

            async move {
                let _permit = semaphore.acquire().await;
                run_task(archive, metadata, providers, split_size).await
            }
        })
        .buffer_unordered(*MAX_THREADS);

    // Execute the tasks
    let result: Vec<Result<(), BackupError>> = tasks.collect().await;

    // Check if there are any errors
    for res in result {
        if res.is_err() {
            return Err(res.err().unwrap());
        }
    }

    let duration = now.elapsed().unwrap_or_default();
    info!("Backup uploaded in {:.2} seconds", duration.as_secs_f64());

    Ok(())
}

/// Computes the maximum number of parts
///
/// # Arguments
///
/// * `file_length` - The length of the file
/// * `split_size` - The size of the split
///
/// # Returns
///
/// The maximum number of parts
fn compute_maximum_parts_number(file_length: u64, split_size: u64) -> u64 {
    let mut max_parts = file_length / split_size;
    if file_length % split_size != 0 {
        max_parts += 1;
    }
    max_parts
}

/// Runs the task to upload the backup
///
/// # Arguments
///
/// * `archive` - The archive to upload
/// * `metadata` - The metadata of the backup
/// * `providers` - The list of storage providers
/// * `split_size` - The size of the split
/// * `semaphore` - The semaphore to limit the number of concurrent tasks
///
/// # Returns
///
/// An empty result
async fn run_task(
    archive: String,
    metadata: Arc<RwLock<BackupMetadata>>,
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
    split_size: u64,
) -> Result<(), BackupError> {
    debug!("Uploading archive '{}'", archive);

    let mut readable_file = make_readable_file(&archive).await?;
    let file_length = readable_file.metadata.len();

    // Calculate the number of parts to derive the padding length
    let max_parts = compute_maximum_parts_number(file_length, split_size);
    let padding_length = max_parts.to_string().len();

    let mut part_index = 1;
    let mut total_bytes_read = 0;

    // Split the file into parts, calculate the hash of each part and upload it
    loop {
        // Read the next fragment
        let fragment = read_buf(&mut readable_file, split_size, total_bytes_read).await;

        // Check if the fragment is an error, if it is, break the loop
        if fragment.is_err() {
            break;
        }
        let fragment = fragment.unwrap();

        // Update the total bytes read
        total_bytes_read = fragment.total_bytes_read;

        // Calculate the hash of the part
        let hash = sha3(&fragment.data);

        // Upload the part
        try_with_valid_provider(
            fragment.data.clone(),
            providers.clone(),
            |provider, buffer| {
                let signature = hash.clone();
                let metadata = metadata.clone();

                Box::pin(async move {
                    // Create the part metadata
                    let part = BackupPart {
                        size: buffer.len() as u64,
                        path: format!(
                            "{}.gitup.part{}",
                            create_id(),
                            pad_number(part_index, padding_length)
                        ),
                        signature,
                        provider: provider.to_string(),
                    };

                    // Upload the part
                    let response = provider.upload(part.path.clone(), buffer.clone()).await;

                    if response.is_ok() {
                        // Update the metadata with the part
                        let mut metadata = metadata.write().await;
                        metadata.parts.push(part);
                        drop(metadata);
                    }

                    response
                })
            },
        )
        .await?;

        part_index += 1;
    }

    Ok(())
}

/// Tries to upload the part to a valid provider
///
/// # Arguments
///
/// * `buffer` - The buffer to upload
/// * `providers` - The list of storage providers
/// * `function` - The function to execute (upload the part)
///
/// # Returns
///
/// An empty result
async fn try_with_valid_provider(
    buffer: Arc<Vec<u8>>,
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
    function: impl Fn(
        Box<dyn StorageProvider>,
        Arc<Vec<u8>>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ProviderError>> + Send>>,
) -> Result<(), BackupError> {
    let buffer_size = buffer.len();

    // Acquire the lock and get the list of providers
    let original_providers = providers.clone();
    let mut providers = providers.read().await;

    // Track the index of the provider and load the first one
    let mut provider_index = 0;
    let mut provider = providers.first().unwrap().clone();

    // Function to get the next provider
    let mut next_provider = async |mut provider_index: &mut usize,
                                   providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>| {
        let providers = providers.read().await;

        *provider_index += 1;
        if let Some(next_provider) = providers.get(*provider_index) {
            Ok(next_provider.clone())
        }
        else {
            error!("All providers failed to store the archive");
            Err(BackupError::NotEnoughSpace)
        }
    };

    // Initialize the failed flag to true, so that the loop runs at least once
    let mut failed = true;

    while failed && provider_index < providers.len() {
        let buffer = buffer.clone();

        // Check if the provider has enough space, if not, move to the next one
        let available_space = provider.get_available_space();
        if available_space < buffer_size as u64 {
            warn!(
                "Provider '{}' does not have enough space to store the archive (available: {}, required: {}), moving \
                 to the next provider",
                provider.name(),
                format_bytesize(available_space),
                format_bytesize(buffer_size as u64)
            );

            provider = next_provider(&mut provider_index, original_providers.clone()).await?;
            continue;
        }

        let response = function(provider.clone(), buffer.clone()).await;
        if response.is_err() {
            error!(
                "Failed to upload part to provider '{}': {}",
                provider.name(),
                response.as_ref().err().unwrap()
            );
            warn!("Trying to upload the part to the next provider");

            provider = next_provider(&mut provider_index, original_providers.clone()).await?;
            continue;
        }

        debug!("Part uploaded to provider '{}'", provider.name());

        failed = false;
    }

    Ok(())
}
