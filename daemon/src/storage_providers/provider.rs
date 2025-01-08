use async_trait::async_trait;
use std::fmt::Display;
use std::sync::Arc;

#[async_trait]
pub trait StorageProvider: Display + Send + Sync {
    /// The name of the storage provider
    fn name(&self) -> &str;

    /// The URL format for the storage provider specialization of:
    /// `gitup://<auth-token>:<provider-name>/<provider-dependant-fragments>`
    fn url(&self) -> &str;

    /// Check the connection to the storage provider
    async fn check_connection(&mut self) -> Result<(), ProviderError>;

    /// A method for cloning the trait object
    fn clone_box(&self) -> Box<dyn StorageProvider>;

    /// Upload the given data to the storage provider at the given path
    async fn upload(&self, path: String, data: Arc<Vec<u8>>) -> Result<(), ProviderError>;

    /// Get the available space on the storage provider in bytes
    fn get_available_space(&self) -> u64;
}

pub trait CreatableStorageProvider: StorageProvider {
    /// Check if the given URL is a provider URL
    fn is_provider_url(url: &str) -> bool
                       where Self: Sized;

    /// Initialize the storage provider with the given URL
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to initialize the storage provider with
    fn new(url: &str) -> Result<Self, ProviderError>
           where Self: Sized;
}

/// Implement `Clone` for `Box<dyn StorageProvider>` using `clone_box`
impl Clone for Box<dyn StorageProvider> {
    fn clone(&self) -> Box<dyn StorageProvider> {
        self.clone_box()
    }
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
