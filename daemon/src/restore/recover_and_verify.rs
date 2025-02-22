use std::{env, mem, ops::Deref, sync::Arc};

use futures::{stream, StreamExt};
use tokio::{
    fs::File,
    io::AsyncWriteExt,
    sync::{RwLock, RwLockReadGuard, Semaphore},
};
use tracing::{debug, info};

use crate::{
    backup::{
        compression::uncompress,
        encrypt::{decrypt, derive_key, key_parts_from_string},
        errors::BackupError,
        structures::BackupMetadata,
    },
    configuration::{Args, MAX_THREADS},
    hash::sha3,
    restore::utility::{check_signature, find_active_provider},
    storage_providers::{provider::StorageProvider, PROVIDER_LIST},
};

/// Recover and verify the provided key
///
/// # Arguments
///
/// * `args` - The command line arguments
/// * `metadata` - The backup metadata
///
/// # Returns
///
/// The key if any
fn get_key_if_any(args: &Args, metadata: Arc<BackupMetadata>) -> Result<Option<[u8; 32]>, BackupError> {
    let provided_key = args.key.clone();
    let snapshot_key = metadata.key.as_ref().unwrap();

    if provided_key.is_none() && metadata.encrypted {
        return Err(BackupError::NoKeyProvided);
    }

    if provided_key.is_some() {
        let provided_key = provided_key.unwrap();
        let parts = key_parts_from_string(snapshot_key);

        let real_key = derive_key(provided_key.as_str(), Some(parts.1.as_bytes()))?;

        let mut hash_vector = Vec::new();
        hash_vector.extend_from_slice(&real_key);
        hash_vector.extend_from_slice(parts.1.as_bytes());
        let hash = sha3(&hash_vector);

        if hash != parts.0 {
            return Err(BackupError::InvalidKey);
        }

        return Ok(Some(real_key));
    }

    Ok(None)
}

/// Recover and verify the backup files from the remote providers
///
/// # Arguments
///
/// * `args` - The command line arguments
/// * `metadata` - The backup metadata
/// * `providers` - The list of storage providers
///
/// # Returns
///
/// The result of the operation
pub async fn recover_and_verify(
    args: &Args,
    metadata: Arc<BackupMetadata>,
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
) -> Result<Vec<String>, BackupError> {
    // Limit the number of concurrent tasks
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    // Regex to extract the URL from the provider, compiled one time only
    let rex = Arc::new(regex::Regex::new(r".+\((?<url>.+)\)").unwrap());

    // Get the key if any
    let key = Arc::new(get_key_if_any(args, metadata.clone())?);

    let tasks = stream::iter(metadata.parts.iter())
        .map(|part| {
            let semaphore = semaphore.clone();
            let providers = providers.clone();
            let rex = rex.clone();
            let temp = env::temp_dir();
            let metadata = metadata.clone();
            let key = key.clone();

            async move {
                let _permit = semaphore.acquire().await;
                // Extract the URL from the provider
                let url = match rex.captures(&part.provider) {
                    Some(caps) => caps["url"].to_string(),
                    None => return Err(BackupError::InvalidSnapshotFile),
                };

                // Find the active provider for the URL
                let providers = providers.read().await;
                let provider = find_active_provider(&providers, &url);
                if provider.is_none() {
                    return Err(BackupError::NoMatchingProvider);
                }
                let provider = provider.unwrap();

                // Download the part
                info!(
                    "Downloading part '{}' from provider '{}'",
                    part.path,
                    provider.to_secret_url()
                );
                let mut data = provider
                    .download(part.path.clone(), part.size)
                    .await
                    .map_err(|e| BackupError::GeneralError(e.to_string()))?;

                // Check the signature of the data
                if !check_signature(part.signature.as_str(), &data) {
                    return Err(BackupError::InvalidSignature);
                }

                // Decrypt the data if needed
                if metadata.encrypted {
                    let key = key.as_ref().unwrap();
                    data = decrypt(data, key).await?;
                }

                // Write the data to the file
                let file_path = temp.join(&part.path);
                File::create(file_path.clone())
                    .await
                    .map_err(|e| BackupError::CannotWrite(e.to_string()))?
                    .write_all(&data)
                    .await
                    .map_err(|e| BackupError::CannotWrite(e.to_string()))?;

                uncompress(file_path.display().to_string(), false).await
            }
        })
        .buffer_unordered(*MAX_THREADS);

    // Execute the tasks
    let result: Vec<Result<String, BackupError>> = tasks.collect().await;

    let mut filenames = Vec::new();

    for res in result {
        filenames.push(res?);
    }

    Ok(filenames)
}
