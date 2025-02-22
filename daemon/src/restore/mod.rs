mod recover_and_verify;
mod utility;

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
    restore::recover_and_verify::recover_and_verify,
    storage_providers::{provider::StorageProvider, PROVIDER_LIST},
};

pub async fn restore(args: &Args, providers: Arc<RwLock<Vec<Box<dyn StorageProvider>>>>) -> Result<(), BackupError> {
    let now = SystemTime::now();

    // Check if the restore_from argument is set
    if args.restore_from.is_none() {
        return Err(BackupError::RestoreFromNotSet);
    }

    let metadata = Arc::new(query_snapshot(args.restore_from.as_ref().unwrap().clone(), false, false).await?);

    // TODO: handle restore from incremental backup

    // Recover and verify the backup parts
    let pieces = recover_and_verify(args, metadata.clone(), providers.clone()).await?;

    let temp = env::temp_dir();

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
