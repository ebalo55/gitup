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
}

impl Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::MalformedPattern(p) => {
                write!(f, "The provided glob pattern '{}' is malformed", p)
            },
            BackupError::CannotReadFile(e) => {
                write!(f, "The file could not be read: {}", e)
            },
            BackupError::CannotCreateBackupFile(e) => {
                write!(f, "The backup file could not be created: {}", e)
            },
            BackupError::CannotAddZipEntry(e) => {
                write!(
                    f,
                    "The backup file could not be added to the zip archive: {}",
                    e
                )
            },
            BackupError::CannotWriteToZipArchive(e) => {
                write!(
                    f,
                    "The backup file could not be written to the zip archive: {}",
                    e
                )
            },
            BackupError::CannotFinalizeZipArchive(e) => {
                write!(f, "The zip archive could not be finalized: {}", e)
            },
            BackupError::GeneralError(e) => {
                write!(f, "General error: {}", e)
            },
            BackupError::CannotEncrypt(e) => {
                write!(f, "Cannot proceed with the encryption operations: {}", e)
            },
            BackupError::NotEnoughSpace => {
                write!(
                    f,
                    "All providers failed or the storage providers do not have enough space to store the backup(s)"
                )
            },
            BackupError::CannotWrite(e) => {
                write!(f, "An error occurred while writing a file: {}", e)
            },
        }
    }
}
