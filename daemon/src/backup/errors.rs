use std::fmt::Display;

/// The error type for the backup execution
#[derive(Debug)]
pub enum BackupError {
    /// The provided glob pattern is malformed
    MalformedPattern(String),
    /// The file could not be read
    CannotReadFile(String),
    /// The backup file could not be created
    CannotCreateBackupFile(String),
    /// The backup file could not be added to the zip archive
    CannotAddZipEntry(String),
    /// The backup file could not be written to the zip archive
    CannotWriteToZipArchive(String),
    /// The zip archive could not be finalized
    CannotFinalizeZipArchive(String),
    /// General error
    GeneralError(String),
    /// Cannot proceed with the encryption operations
    CannotEncrypt(String),
    /// The storage providers do not have enough space to store the backup(s)
    NotEnoughSpace,
    /// An error occurred while writing a file
    CannotWrite(String),
    /// Cannot restore from the snapshot file
    SnapshotFileError(String),
    /// The restore_from argument is not set
    RestoreFromNotSet,
    /// The part is invalid
    InvalidSnapshotFile,
    /// The provider is unknown
    NoMatchingProvider,
}

impl Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::MalformedPattern(p) => {
                write!(f, "The provided glob pattern '{}' is malformed", p)
            },
            Self::CannotReadFile(e) => {
                write!(f, "The file could not be read: {}", e)
            },
            Self::CannotCreateBackupFile(e) => {
                write!(f, "The backup file could not be created: {}", e)
            },
            Self::CannotAddZipEntry(e) => {
                write!(
                    f,
                    "The backup file could not be added to the zip archive: {}",
                    e
                )
            },
            Self::CannotWriteToZipArchive(e) => {
                write!(
                    f,
                    "The backup file could not be written to the zip archive: {}",
                    e
                )
            },
            Self::CannotFinalizeZipArchive(e) => {
                write!(f, "The zip archive could not be finalized: {}", e)
            },
            Self::GeneralError(e) => {
                write!(f, "General error: {}", e)
            },
            Self::CannotEncrypt(e) => {
                write!(f, "Cannot proceed with the encryption operations: {}", e)
            },
            Self::NotEnoughSpace => {
                write!(
                    f,
                    "All providers failed or the storage providers do not have enough space to store the backup(s)"
                )
            },
            Self::CannotWrite(e) => {
                write!(f, "An error occurred while writing a file: {}", e)
            },
            Self::InvalidSnapshotFile => {
                write!(f, "One or more parts in the snapshot file are invalid")
            },
            Self::SnapshotFileError(e) => {
                write!(f, "Cannot restore from the snapshot file: {}", e)
            },
            Self::RestoreFromNotSet => {
                write!(f, "The restore_from argument is not set")
            },
            BackupError::NoMatchingProvider => {
                write!(
                    f,
                    "Cannot proceed with the restore operation: no matching provider found in current configuration"
                )
            },
        }
    }
}
