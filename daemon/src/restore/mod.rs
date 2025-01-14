use std::{
    env,
    fs,
    fs::{create_dir, create_dir_all, remove_file},
    sync::Arc,
    time::SystemTime,
};

use cuid2::create_id;
use futures::{stream, StreamExt};
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
    sync::{RwLock, RwLockReadGuard, Semaphore},
};
use tracing::{debug, info};

use crate::{
    backup::{
        compression::uncompress,
        errors::BackupError,
        file::{make_readable_file, make_readable_file_std, open_file_or_fail, read_buf, FileMode},
        query_snapshot,
    },
    byte_size::format_bytesize,
    configuration::{Args, DEFAULT_BUFFER_SIZE, MAX_THREADS},
    storage_providers::{provider::StorageProvider, PROVIDER_LIST},
};

pub async fn restore(args: &Args, providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>) -> Result<(), BackupError> {
    let now = SystemTime::now();

    // Check if the restore_from argument is set
    if args.restore_from.is_none() {
        return Err(BackupError::RestoreFromNotSet);
    }

    let metadata = query_snapshot(args.restore_from.as_ref().unwrap().clone(), false, false).await?;

    // TODO: handle restore from incremental backup

    // Limit the number of concurrent tasks
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    let rex = Arc::new(regex::Regex::new(r".+\((?<url>.+)\)").unwrap());
    let temp = env::temp_dir();

    let tasks = stream::iter(metadata.parts.iter())
        .map(|part| {
            let semaphore = semaphore.clone();
            let providers = providers.clone();
            let rex = rex.clone();
            let temp = env::temp_dir();

            async move {
                let _permit = semaphore.acquire().await;
                let url = match rex.captures(&part.provider) {
                    Some(caps) => caps["url"].to_string(),
                    None => return Err(BackupError::InvalidSnapshotFile),
                };

                let providers = providers.read().await;
                let provider = find_active_provider(&providers, &url);
                if provider.is_none() {
                    return Err(BackupError::NoMatchingProvider);
                }
                let provider = provider.unwrap();

                info!(
                    "Downloading part '{}' from provider '{}'",
                    part.path,
                    provider.to_secret_url()
                );
                let data = provider
                    .download(part.path.clone(), part.size)
                    .await
                    .map_err(|e| BackupError::GeneralError(e.to_string()))?;

                // TODO: handle signature check
                // TODO: handle decryption

                let file_path = temp.join(&part.path);
                // Write the data to the file
                File::create(file_path.clone())
                    .await
                    .map_err(|e| BackupError::CannotWrite(e.to_string()))?
                    .write_all(&data)
                    .await
                    .map_err(|e| BackupError::CannotWrite(e.to_string()))?;

                uncompress(file_path.display().to_string(), false).await?;

                Ok(())
            }
        })
        .buffer_unordered(*MAX_THREADS);

    // Execute the tasks
    let result: Vec<Result<(), BackupError>> = tasks.collect().await;
    for res in result {
        if res.is_err() {
            return Err(res.err().unwrap());
        }
    }

    let final_backup_path = temp
        .join(format!("{}.gitup", create_id()))
        .display()
        .to_string();
    let final_backup = open_file_or_fail(final_backup_path.as_str(), FileMode::Create)
        .await
        .map_err(|e| BackupError::CannotWrite(e.to_string()))?;
    let mut writer = BufWriter::new(final_backup);

    for part in metadata.parts.iter() {
        let archive_path = temp.join(format!("{}uz", part.path));
        if !archive_path.exists() {
            return Err(BackupError::CannotReadFile(format!(
                "Cannot read file at '{}'",
                archive_path.display().to_string()
            )));
        }
        let archive_path = archive_path.display().to_string();
        debug!("Reading file '{}'", archive_path);

        let mut readable_file = make_readable_file(archive_path.as_str()).await?;
        let mut total_bytes_read = 0;

        // Read the file in chunks and write it to the final backup file
        loop {
            let fragment = read_buf(&mut readable_file, DEFAULT_BUFFER_SIZE, total_bytes_read).await;

            if fragment.is_err() {
                break;
            }
            let fragment = fragment.unwrap();

            total_bytes_read = fragment.total_bytes_read;

            debug!(
                "Writing {} of data to final backup file at '{}'",
                format_bytesize(fragment.data.len() as u64),
                final_backup_path
            );

            writer
                .write_all(&fragment.data)
                .await
                .map_err(|e| BackupError::CannotWrite(e.to_string()))?;
        }

        remove_file(archive_path).map_err(|e| BackupError::CannotWrite(e.to_string()))?;
    }

    writer
        .flush()
        .await
        .map_err(|e| BackupError::CannotWrite(e.to_string()))?;
    drop(writer);

    info!("Archive ready, restoring ...");
    create_dir_all("restore").map_err(|e| BackupError::CannotWrite(e.to_string()))?;

    let reader = make_readable_file_std(final_backup_path.as_str()).await?;
    let mut zip = zip::ZipArchive::new(reader.reader).map_err(|e| BackupError::CannotReadFile(e.to_string()))?;

    // Restore the files
    for i in 0 .. zip.len() {
        let mut zip_file = zip
            .by_index(i)
            .map_err(|e| BackupError::CannotReadFile(e.to_string()))?;
        let file_path = zip_file.enclosed_name().ok_or(BackupError::CannotReadFile(
            "Cannot read file name as it is malformed".to_string(),
        ))?;

        debug!("Restoring file '{}'", file_path.display());

        let file_path = file_path
            .to_string_lossy()
            .replace(":", "")
            .replace("//", "/")
            .replace("\\", "/")
            .replace("?", "")
            .replace("///", "");
        let file_path = format!("restore/{}", file_path);
        let file_path = env::current_dir().unwrap().join(file_path);
        let parent = file_path.parent().unwrap();
        create_dir_all(parent).map_err(|e| BackupError::CannotWrite(e.to_string()))?;

        debug!("Writing file to '{}'", file_path.display());

        let mut file = fs::File::create(file_path).map_err(|e| BackupError::CannotWrite(e.to_string()))?;
        std::io::copy(&mut zip_file, &mut file).map_err(|e| BackupError::CannotWrite(e.to_string()))?;
    }

    for (_, meta) in metadata.tree.iter() {
        for (_, file_meta) in &meta.files {
            // Check if the file has duplicates, if not, skip it
            if file_meta.duplicates.is_empty() {
                continue;
            }

            let file_path = format!("./restore/{}", file_meta.original_path);
            debug!("Restoring duplicates for file '{}'", file_path);

            for duplicate in &file_meta.duplicates {
                let duplicate_path = format!("./restore/{}", duplicate.display());

                if file_path == duplicate_path {
                    continue;
                }

                debug!(
                    "Duplicating original file at '{}' to '{}'",
                    file_path, duplicate_path
                );
                fs::copy(&file_path, &duplicate_path).map_err(|e| BackupError::CannotWrite(e.to_string()))?;
            }
        }
    }

    info!(
        "Restore completed in {:.2} seconds",
        now.elapsed().unwrap().as_secs_f64()
    );

    Ok(())
}

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
fn find_active_provider<'a>(
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
