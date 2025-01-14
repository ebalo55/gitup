use std::{fmt::Display, sync::Arc};

use async_trait::async_trait;

#[async_trait]
pub trait StorageProvider: Display + Send + Sync {
    /// The name of the storage provider
    fn name(&self) -> &str;

    /// The URL format for the storage provider specialization of:
    /// `gitup://<auth-token>:<provider-name>/<provider-dependant-fragments>`
    fn url(&self) -> &str;

    /// Returns the secret URL for the storage provider where the auth token is hidden
    fn to_secret_url(&self) -> String;

    /// Check the connection to the storage provider
    async fn check_connection(&mut self) -> Result<(), ProviderError>;

    /// A method for cloning the trait object
    fn clone_box(&self) -> Box<dyn StorageProvider>;

    /// Upload the given data to the storage provider at the given path
    ///
    /// # Arguments
    ///
    /// * `path` - The path to upload the data to
    /// * `data` - The data to upload
    ///
    /// # Returns
    ///
    /// `Ok(())` if the upload was successful, an error otherwise
    async fn upload(&self, path: String, data: Arc<Vec<u8>>) -> Result<(), ProviderError>;

    /// Download the data from the storage provider at the given path
    ///
    /// # Arguments
    ///
    /// * `path` - The path to download the data from
    /// * `expected_size` - The expected size of the data to download in bytes
    ///
    /// # Returns
    ///
    /// The downloaded data as a byte vector
    async fn download(&self, path: String, expected_size: u64) -> Result<Arc<Vec<u8>>, ProviderError>;

    /// Get the available space on the storage provider in bytes
    fn get_available_space(&self) -> u64;
}

pub trait CreatableStorageProvider: StorageProvider + Default {
    /// Check if the given URL is a provider URL
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to check
    ///
    /// # Returns
    ///
    /// `true` if the URL is a provider URL, `false` otherwise
    fn is_provider_url(url: &str) -> bool
    where
        Self: Sized;

    /// Initialize the storage provider with the given URL
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to initialize the storage provider with
    ///
    /// # Returns
    ///
    /// A new instance of the storage provider
    fn new(url: &str) -> Result<Box<dyn StorageProvider>, ProviderError>
    where
        Self: Sized;

    /// Print the provider information
    fn print_provider_info() {
        let instance = Self::default();
        println!("Provider: {}", instance.name());
        println!("\t- URL: {}", instance.url());
    }
}

/// Implement `Clone` for `Box<dyn StorageProvider>` using `clone_box`
impl Clone for Box<dyn StorageProvider> {
    fn clone(&self) -> Box<dyn StorageProvider> { self.clone_box() }
}

/// The error type for the storage provider
#[derive(Debug)]
pub enum ProviderError {
    /// The provider is invalid or not supported
    UnknownProvider(String),
    /// The provider URL is invalid and cannot be parsed
    InvalidProviderUrl,
    /// An error occurred while connecting to the provider
    ConnectionError(String),
    /// The provider failed to fullfill a precondition
    PreconditionsFailed(String),
    /// Generic error
    GenericError(String),
}

impl Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::UnknownProvider(p) => {
                write!(
                    f,
                    "The provider is invalid or not supported, provider: {}",
                    p
                )
            },
            Self::InvalidProviderUrl => write!(f, "The provider URL is invalid and cannot be parsed"),
            Self::ConnectionError(e) => {
                write!(
                    f,
                    "An error occurred while connecting to the provider: {}",
                    e
                )
            },
            Self::PreconditionsFailed(e) => write!(f, "The provider failed to fullfill a precondition: {}", e),
            Self::GenericError(e) => write!(f, "Generic error: {}", e),
        }
    }
}
