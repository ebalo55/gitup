use crate::storage_providers::provider::{CreatableStorageProvider, ProviderError, StorageProvider};

pub mod provider;
pub mod github;

/// Initialize the storage providers with the given list of provider URLs
///
/// # Arguments
///
/// * `providers` - The list of provider URLs to initialize
///
/// # Returns
///
/// A vector of initialized storage providers
pub fn init_providers(providers: &Vec<String>) -> Result<Vec<Box<dyn StorageProvider>>, ProviderError> {
    let mut provider_list: Vec<Box<dyn StorageProvider>> = Vec::new();

    for provider in providers {
        if github::Provider::is_provider_url(&provider) {
            provider_list.push(Box::new(github::Provider::new(provider.as_str())?));
        } else {
            return Err(ProviderError::UnknownProvider(provider.as_str().to_string()));
        }
    }

    Ok(provider_list)
}
