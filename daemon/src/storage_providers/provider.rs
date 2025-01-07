pub trait StorageProvider {
    /// The name of the storage provider
    fn name(&self) -> &str;

    /// The description of the storage provider
    fn description(&self) -> &str;

    /// The configuration options for the storage provider
    fn configuration(&self) -> Vec<ConfigurationOption>;

    /// Initialize the storage provider
    fn initialize(&self, config: &HashMap<String, String>) -> Result<Box<dyn StorageProviderInstance>, Box<dyn Error>>;
}
