use std::{
    io::{BufReader, Read},
    sync::Arc,
    time::SystemTime,
};

use flate2::{
    bufread::{ZlibDecoder, ZlibEncoder},
    Compression,
};
use futures::{future::join_all, stream, StreamExt};
use tokio::{
    fs::{remove_file, File},
    io::{AsyncWriteExt, BufWriter},
    sync::{RwLock, Semaphore},
};
use tracing::{debug, error, info, warn};

use crate::{
    backup::{
        errors::BackupError,
        file::{make_readable_file, make_readable_file_std, open_file_or_fail, FileError, FileMode},
        file_size,
        structures::BackupMetadata,
        utility::compute_size_variation,
    },
    byte_size::format_bytesize,
    configuration::{DEFAULT_BUFFER_SIZE, MAX_THREADS},
};

/// Compresses the archives
///
/// # Arguments
///
/// * `archives` - The archives to compress
/// * `compression` - The compression algorithm to use
///
/// # Returns
///
/// A list of paths to the compressed archives
pub async fn compress_archives(
    archives: Vec<String>,
    metadata: Arc<RwLock<BackupMetadata>>,
    compression: Compression,
) -> Result<Vec<String>, BackupError> {
    let now = SystemTime::now();

    let compression = Arc::new(compression);

    // Limit the number of concurrent tasks
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    let tasks = stream::iter(archives.into_iter())
        .map(|archive| {
            let compression = compression.clone();
            let semaphore = semaphore.clone();

            async move {
                let _permit = semaphore.acquire().await;
                compress(archive, compression).await
            }
        })
        .buffer_unordered(*MAX_THREADS);

    // Execute the tasks
    let result: Vec<Result<String, BackupError>> = tasks.collect().await;
    let (compressed_size, compressed_archives) = ensure_no_errors_counting_size(result)?;

    // Update the metadata with the stats
    let original_size = {
        let mut metadata = metadata.write().await;
        metadata.stats.compressed_size = compressed_size;
        metadata.stats.archival_size
    };

    let duration = now.elapsed().unwrap_or_default();

    info!(
        "Compressed {} archive(s) in {:.2} seconds ({}, {})",
        compressed_archives.len(),
        duration.as_secs_f64(),
        format_bytesize(compressed_size),
        compute_size_variation(original_size as f64, compressed_size as f64)
    );

    Ok(compressed_archives)
}

/// Ensures that no errors occurred while operating on files returning the total size and the list
/// of files
///
/// # Arguments
///
/// * `result` - The result of the file size computation
///
/// # Returns
///
/// The total size of the files and the list of files
pub fn ensure_no_errors_counting_size(
    result: Vec<Result<String, BackupError>>,
) -> Result<(u64, Vec<String>), BackupError> {
    let mut container = Vec::new();
    let mut size = 0;

    for res in result {
        if res.is_err() {
            return Err(BackupError::GeneralError(res.err().unwrap().to_string()));
        }
        let res = res.unwrap();

        container.push(res.clone());
        size += file_size(res.as_str()).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;
    }

    Ok((size, container))
}

/// Compresses a file
///
/// # Arguments
///
/// * `file_path` - The path to the file
/// * `compression` - The compression algorithm to use
///
/// # Returns
///
/// The path to the compressed file
pub async fn compress(file_path: String, compression: Arc<Compression>) -> Result<String, BackupError> {
    use std::io::Read;

    // Open the archive
    let file = make_readable_file_std(file_path.as_str()).await?;
    let file_length = file.metadata.len();

    // Create the compressed archive
    let compressed_filename = format!("{}z", file_path);
    let compressed_file = open_file_or_fail(compressed_filename.as_str(), FileMode::Create)
        .await
        .map_err(|e| {
            error!("Failed to compress '{}': {}", compressed_filename, e);

            BackupError::CannotCreateBackupFile(e.to_string())
        })?;
    let mut writer = BufWriter::new(compressed_file);

    // prepare the compressor, reader and writer
    let mut compressor = ZlibEncoder::new(file.reader, *compression);

    let mut total_bytes_read = 0;

    // read the file and write it to the compressed file in chunks of COMPRESSION_BUFFER_SIZE
    loop {
        // Ensure we don't read beyond the file's total size
        if total_bytes_read >= file_length {
            break;
        }

        // Calculate the remaining bytes to read
        let remaining_bytes = (file_length - total_bytes_read) as usize;

        // Define the buffer size as the minimum between the remaining bytes and the split size
        let buffer_size = remaining_bytes.min(DEFAULT_BUFFER_SIZE as usize);

        // create the buffer to store the part
        let mut buffer = vec![0; buffer_size];

        // read the part
        let bytes_read = compressor
            .read(&mut buffer)
            .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

        if bytes_read == 0 {
            break;
        }
        buffer.truncate(bytes_read);

        writer
            .write(&buffer)
            .await
            .map_err(|e| BackupError::CannotWrite(e.to_string()))?;

        // Update the total bytes read
        total_bytes_read = compressor.total_in();
    }

    // remove the original archive
    remove_file(file_path)
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    Ok(compressed_filename)
}

pub async fn uncompress(file_path: String) -> Result<String, BackupError> {
    use std::io::Read;

    // Open the archive
    let file = make_readable_file_std(file_path.as_str()).await?;
    let file_length = file.metadata.len();

    // Create the compressed archive
    let compressed_filename = format!("{}uz", file_path);
    let compressed_file = open_file_or_fail(compressed_filename.as_str(), FileMode::Create)
        .await
        .map_err(|e| {
            error!("Failed to compress '{}': {}", compressed_filename, e);

            BackupError::CannotCreateBackupFile(e.to_string())
        })?;
    let mut writer = BufWriter::new(compressed_file);

    // prepare the compressor, reader and writer
    let mut compressor = ZlibDecoder::new(file.reader);

    let mut total_bytes_read = 0;

    // read the file and write it to the compressed file in chunks of COMPRESSION_BUFFER_SIZE
    loop {
        // Ensure we don't read beyond the file's total size
        if total_bytes_read >= file_length {
            break;
        }

        // Calculate the remaining bytes to read
        let remaining_bytes = (file_length - total_bytes_read) as usize;

        // Define the buffer size as the minimum between the remaining bytes and the split size
        let buffer_size = remaining_bytes.min(DEFAULT_BUFFER_SIZE as usize);

        // create the buffer to store the part
        let mut buffer = vec![0; buffer_size];

        // read the part
        let bytes_read = compressor
            .read(&mut buffer)
            .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

        if bytes_read == 0 {
            break;
        }
        buffer.truncate(bytes_read);

        writer
            .write(&buffer)
            .await
            .map_err(|e| BackupError::CannotWrite(e.to_string()))?;

        // Update the total bytes read
        total_bytes_read = compressor.total_in();
    }

    // remove the original archive
    remove_file(file_path)
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    Ok(compressed_filename)
}

#[cfg(test)]
mod test {
    use crate::backup::compression::uncompress;

    #[tokio::test]
    async fn test_uncompress() {
        uncompress("C:\\Users\\ebalo\\AppData\\Local\\Temp\\otemxlsjl30gtx2ei73tukag.gitupz".to_string())
            .await
            .unwrap();
    }
}
