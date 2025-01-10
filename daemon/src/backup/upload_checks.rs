use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::{backup::errors::BackupError, byte_size::format_bytesize, storage_providers::provider::StorageProvider};

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
pub async fn check_available_size(
    providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>,
    size: u64,
) -> Result<(), BackupError> {
    let providers = providers.read().await;

    let mut available_size = 0;
    for provider in providers.iter() {
        available_size += provider.get_available_space();
    }

    if available_size < size {
        debug!(
            "Available space: {}, Required space: {}",
            format_bytesize(available_size),
            format_bytesize(size)
        );
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
