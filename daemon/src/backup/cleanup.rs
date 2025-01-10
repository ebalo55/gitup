use std::{env, fs};

use glob::glob;
use tracing::{debug, error};

/// Cleans up the temporary folder
///
/// This function will remove all the files in the temporary folder that have been created during
/// the backup process
pub fn cleanup_temp_folder() {
    debug!("Cleaning up temporary folder");

    for entry in glob(&format!("{}/*.gitup*", env::temp_dir().display())).unwrap() {
        match entry {
            Ok(path) => {
                if path.is_file() {
                    let _ = fs::remove_file(path);
                }
            },
            Err(e) => {
                error!("Failed to read path: {}", e);
            },
        }
    }
}
