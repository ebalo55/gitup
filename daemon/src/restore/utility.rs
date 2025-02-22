use tokio::sync::RwLockReadGuard;
use tracing::debug;

use crate::{
    hash::sha3,
    storage_providers::{provider::StorageProvider, PROVIDER_LIST},
};

/// Find the active provider for the given URL
///
/// # Arguments
///
/// * `providers` - The list of storage providers
/// * `url` - The URL to match
///
/// # Returns
///
/// The active storage provider for the given URL
pub fn find_active_provider<'a>(
    providers: &'a RwLockReadGuard<Vec<Box<dyn StorageProvider>>>,
    url: &'a str,
) -> Option<&'a Box<dyn StorageProvider>> {
    // Check if the URL matches any of the provider URL formats
    for (name, is_provider_url, new) in PROVIDER_LIST.iter() {
        if is_provider_url(url) {
            // loop through the providers to find the one that matches the URL
            for provider in providers.iter() {
                // if the provider name equal the one of the provider matching the loaded url AND creating a new
                // provider with the loaded url matches the loaded url, then we found the correct provider and
                // return it
                //
                // Why should we match the provider name and the URL?
                // - The provider name is used to identify the provider in the provider list but if many instances
                //   of the same provider exists they will all have the same name. Therefore, we need to filter them
                //   again
                // - The URL is used to uniquely identify the provider in the provider list but there may be
                //   multiple providers accepting the same format of url and returning different objects. Therefore
                //   we need to pre-filter them in order to find only the correct providers and avoid useless
                //   comparisons (and failing)
                if provider.name() == name() {
                    let temp_instance = new(url);
                    debug!(
                        "Checking if provider '{}' matches URL '{}'",
                        provider.to_secret_url(),
                        url
                    );
                    if temp_instance.is_ok() && temp_instance.unwrap().to_secret_url() == url {
                        debug!("Provider found: '{}'", url);
                        return Some(provider);
                    }
                }
            }
        }
    }

    None
}

/// Check the signature of the data
///
/// # Arguments
///
/// * `signature` - The signature to check
/// * `data` - The data to check
///
/// # Returns
///
/// True if the signature is correct, false otherwise
pub fn check_signature(signature: &str, data: &[u8]) -> bool {
    let to_check = sha3(data);

    to_check == signature
}
