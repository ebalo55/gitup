use std::{fmt::Display, fs::Metadata, path::PathBuf, sync::Arc};

use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader},
};
use tracing::error;

use crate::backup::errors::BackupError;

/// Represents the error that can occur when reading a file
#[derive(Debug)]
pub enum FileError {
    /// The file reached EOF
    EOF,
    /// The file could not be opened
    CannotOpenFile(String),
    /// The file could not be read
    CannotReadFile(String),
}

impl Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FileError::EOF => write!(f, "The file reached EOF"),
            FileError::CannotOpenFile(e) => write!(f, "The file could not be opened: {}", e),
            FileError::CannotReadFile(e) => write!(f, "The file could not be read: {}", e),
        }
    }
}

/// Represents a readable file
#[derive(Debug)]
pub struct ReadableFile {
    /// The file reader
    pub reader:   BufReader<File>,
    /// The file metadata
    pub metadata: Metadata,
}

/// Represents a readable file
#[derive(Debug)]
pub struct ReadableFileStd {
    /// The file reader
    pub reader:   std::io::BufReader<std::fs::File>,
    /// The file metadata
    pub metadata: Metadata,
}

/// Represents a fragment of a file read
#[derive(Debug)]
pub struct ReadFragment {
    /// The data read
    pub data:             Arc<Vec<u8>>,
    /// The total bytes read
    pub total_bytes_read: u64,
}

/// Represents the file mode to open
pub enum FileMode {
    /// Read mode
    Read,
    /// Write mode
    Write,
    /// Read and write mode
    ReadWrite,
    /// Create mode (write and create if it does not exist)
    Create,
}

/// Opens a file or fails
///
/// # Arguments
///
/// * `path` - The path to the file
/// * `mode` - The file mode
///
/// # Returns
///
/// A file struct
pub async fn open_file_or_fail(path: &str, mode: FileMode) -> Result<File, FileError> {
    let file = match mode {
        FileMode::Read => File::options().read(true).open(path).await,
        FileMode::Write => File::options().write(true).open(path).await,
        FileMode::ReadWrite => File::options().read(true).write(true).open(path).await,
        FileMode::Create => File::options().write(true).create(true).open(path).await,
    };
    if file.is_err() {
        let err = file.err().unwrap();
        error!("Failed to open file '{}': {}", path, err);
        return Err(FileError::CannotOpenFile(err.to_string()));
    }

    Ok(file.unwrap())
}

/// Reads a file or fails
///
/// # Arguments
///
/// * `path` - The path to the file
///
/// # Returns
///
/// A readable file struct
pub async fn make_readable_file(path: &str) -> Result<ReadableFile, BackupError> {
    if !PathBuf::from(path).is_file() {
        return Err(BackupError::CannotReadFile(format!(
            "Path '{}' is not a valid file",
            path
        )));
    }
    let file = open_file_or_fail(path, FileMode::Read)
        .await
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    let file_meta = file
        .metadata()
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    let mut reader = BufReader::new(file);

    Ok(ReadableFile {
        reader,
        metadata: file_meta,
    })
}

/// Reads a file or fails using the standard library
///
/// # Arguments
///
/// * `path` - The path to the file
///
/// # Returns
///
/// A readable file struct
pub async fn make_readable_file_std(path: &str) -> Result<ReadableFileStd, BackupError> {
    if !PathBuf::from(path).is_file() {
        return Err(BackupError::CannotReadFile(format!(
            "Path '{}' is not a valid file",
            path
        )));
    }

    let file = open_file_or_fail(path, FileMode::Read)
        .await
        .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    let file_meta = file
        .metadata()
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    let mut reader = std::io::BufReader::new(file.into_std().await);

    Ok(ReadableFileStd {
        reader,
        metadata: file_meta,
    })
}

/// Reads a part of a file
///
/// # Arguments
///
/// * `readable_file` - The readable file
/// * `max_buffer_size` - The maximum buffer size
/// * `total_bytes_read` - The total bytes read
///
/// # Returns
///
/// A read fragment
pub async fn read_buf(
    mut readable_file: &mut ReadableFile,
    max_buffer_size: u64,
    total_bytes_read: u64,
) -> Result<ReadFragment, FileError> {
    let file_length = readable_file.metadata.len();

    // Ensure we don't read beyond the file's total size
    if total_bytes_read >= file_length {
        return Err(FileError::EOF);
    }

    // Calculate the remaining bytes to read
    let remaining_bytes = (file_length - total_bytes_read) as usize;

    // Define the buffer size as the minimum between the remaining bytes and the split size
    let buffer_size = remaining_bytes.min(max_buffer_size as usize);

    // create the buffer to store the part
    let mut buffer = vec![0; buffer_size];

    // read the part
    let bytes_read = readable_file
        .reader
        .read_exact(&mut buffer)
        .await
        .map_err(|e| FileError::CannotReadFile(e.to_string()))?;

    if bytes_read == 0 {
        return Err(FileError::EOF);
    }

    buffer.truncate(bytes_read);

    Ok(ReadFragment {
        data:             Arc::new(buffer),
        total_bytes_read: total_bytes_read + bytes_read as u64,
    })
}
