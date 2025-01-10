use std::{sync::Arc, time::SystemTime};

use chacha20poly1305::{
    aead::{Aead, OsRng},
    AeadCore,
    Key,
    KeyInit,
    XChaCha20Poly1305,
};
use futures::{future::join_all, stream, StreamExt};
use hkdf::Hkdf;
use sha3::Sha3_512;
use tokio::{
    fs::{remove_file, File},
    io::{AsyncReadExt, AsyncWriteExt, BufWriter},
    sync::{RwLock, Semaphore},
};
use tracing::{debug, error, info};

use crate::{
    backup::{
        compression::{compress, ensure_no_errors_counting_size},
        errors::BackupError,
        file::{make_readable_file, open_file_or_fail, read_buf, FileMode},
        structures::BackupMetadata,
        utility::compute_size_variation,
    },
    byte_size::format_bytesize,
    configuration::{DEFAULT_BUFFER_SIZE, MAX_THREADS},
    hash::sha3,
};

/// Encrypts the archives using the provided key
///
/// # Arguments
///
/// * `key` - The encryption key
/// * `archives` - The archives to encrypt
/// * `metadata` - The metadata of the backup
///
/// # Returns
///
/// A list of paths to the encrypted archives
pub async fn encrypt_archives(
    key: &String,
    archives: Vec<String>,
    metadata: Arc<RwLock<BackupMetadata>>,
) -> Result<Vec<String>, BackupError> {
    let now = SystemTime::now();

    // Generate the encryption key and nonce
    let hkdf_salt = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let hkdf = Hkdf::<Sha3_512>::new(Some(hkdf_salt.as_slice()), key.as_bytes());
    let mut key = [0u8; 32];
    hkdf.expand(b"gitup", &mut key)
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    // Hash the key and store it in the metadata
    let mut hash_data = Vec::new();
    hash_data.extend_from_slice(key.as_slice());
    hash_data.extend_from_slice(hkdf_salt.as_slice());
    let hash = sha3(hash_data.as_slice());
    let salt = format!("{:x}", hkdf_salt);

    // Update the metadata
    let mut working_metadata = metadata.write().await;
    working_metadata.encrypted = true;
    working_metadata.key = Some(format!("{}{}", hash, salt));
    drop(working_metadata);

    // make the key sharable
    let key = Arc::new(key);

    // Limit the number of concurrent tasks
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    let tasks = stream::iter(archives.into_iter())
        .map(|archive| {
            let key = key.clone();
            let semaphore = semaphore.clone();

            async move {
                let _permit = semaphore.acquire().await;
                encrypt(archive, key).await
            }
        })
        .buffer_unordered(*MAX_THREADS);

    // Execute the tasks
    let result: Vec<Result<String, BackupError>> = tasks.collect().await;
    let (encrypted_size, encrypted_archives) = ensure_no_errors_counting_size(result)?;

    // Update the metadata with the stats
    let original_size = {
        let mut working_metadata = metadata.write().await;
        working_metadata.stats.encrypted_size = encrypted_size;

        // return the size relative to the previous step
        if working_metadata.stats.compressed_size == 0 {
            working_metadata.stats.archival_size
        }
        else {
            working_metadata.stats.compressed_size
        }
    };

    let duration = now.elapsed().unwrap_or_default();

    info!(
        "Encrypted {} archive(s) in {:.2} seconds ({}, {})",
        encrypted_archives.len(),
        duration.as_secs_f64(),
        format_bytesize(encrypted_size),
        compute_size_variation(original_size as f64, encrypted_size as f64)
    );

    Ok(encrypted_archives)
}

/// Encrypts the archive using the provided key
///
/// # Arguments
///
/// * `archive` - The archive to encrypt
/// * `key` - The encryption key
///
/// # Returns
///
/// The path to the encrypted archive
async fn encrypt(archive: String, key: Arc<[u8; 32]>) -> Result<String, BackupError> {
    // Open the archive
    let mut readable_file = make_readable_file(&archive).await?;

    // Create the encrypted archive
    let encrypted_filename = format!("{}c", archive);
    let encrypted_file = open_file_or_fail(&encrypted_filename, FileMode::Create)
        .await
        .map_err(|e| {
            error!(
                "Failed to create encrypted archive '{}': {}",
                encrypted_filename, e
            );

            BackupError::GeneralError(e.to_string())
        })?;

    // prepare cipher, reader and writer
    let mut cipher = XChaCha20Poly1305::new(Key::from_slice(key.as_slice()));
    let mut writer = BufWriter::new(encrypted_file);

    let mut total_bytes_read = 0;

    // read the file and write it to the encrypted file in chunks of ENCRYPTION_BUFFER_SIZE
    loop {
        let fragment = read_buf(&mut readable_file, DEFAULT_BUFFER_SIZE, total_bytes_read).await;

        // Check if the fragment is an error, if it is, break the loop
        if fragment.is_err() {
            break;
        }
        let fragment = fragment.unwrap();

        // Update the total bytes read
        total_bytes_read = fragment.total_bytes_read;

        // generate a nonce and encrypt the buffer
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, fragment.data.as_slice())
            .map_err(|e| BackupError::CannotEncrypt(e.to_string()))?;

        // write the nonce and the ciphertext to the encrypted file
        writer
            .write(&nonce)
            .await
            .map_err(|e| BackupError::CannotEncrypt(e.to_string()))?;
        writer
            .write(&ciphertext)
            .await
            .map_err(|e| BackupError::CannotEncrypt(e.to_string()))?;
    }

    // remove the original archive
    remove_file(archive)
        .await
        .map_err(|e| BackupError::GeneralError(e.to_string()))?;

    Ok(encrypted_filename)
}
