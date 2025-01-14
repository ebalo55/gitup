use std::string::ToString;

use crate::storage_providers::provider::{CreatableStorageProvider, ProviderError, StorageProvider};

pub mod github;
pub mod provider;

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
            provider_list.push(github::Provider::new(provider.as_str())?);
        }
        else {
            return Err(ProviderError::UnknownProvider(
                provider.as_str().to_string(),
            ));
        }
    }

    Ok(provider_list)
}

/// The list of storage providers in a tuple of the provider name and the URL check function
pub static PROVIDER_LIST: [(
    fn() -> String,
    fn(&str) -> bool,
    fn(&str) -> Result<Box<dyn StorageProvider>, ProviderError>,
); 1] = [(
    || github::Provider::default().name().to_string(),
    github::Provider::is_provider_url,
    github::Provider::new,
)];
