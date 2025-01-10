mod backup;
mod base64;
mod byte_size;
mod configuration;
mod hash;
mod padding;
mod storage_providers;

use std::{collections::HashMap, fs, io, mem, path::PathBuf, sync::Arc};

use clap::{Parser, ValueEnum};
use futures::{stream, StreamExt};
use optional_struct::{optional_struct, Applicable};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tracing::{debug, error, field::debug, info, warn, Level};
use tracing_subscriber::{
    fmt::{
        layer,
        writer::{BoxMakeWriter, MakeWriterExt},
    },
    layer,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Registry,
};

use crate::{
    backup::backup,
    configuration::{merge_configuration_file, Args, OperationalMode, OptionalArgs, MAX_THREADS},
    storage_providers::provider::{CreatableStorageProvider, StorageProvider},
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = Args::parse();

    // Print the list of available storage providers and exit
    if args.provider_list {
        storage_providers::github::Provider::print_provider_info();
        return Ok(());
    }

    let config = merge_configuration_file(args)?;

    // Setup tracing
    let log_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("gitup.log")
        .map_err(|e| e.to_string())?;
    Registry::default()
        .with(
            layer()
                .with_writer(io::stdout.with_max_level(
                    if config.debug {
                        Level::DEBUG
                    }
                    else {
                        Level::INFO
                    },
                ))
                .with_ansi(true)
                .with_level(true)
                .with_file(false)
                .with_line_number(false)
                .compact(),
        )
        .with(
            layer()
                .with_writer(log_file.with_max_level(Level::DEBUG))
                .with_ansi(false)
                .with_level(true)
                .with_file(false)
                .with_line_number(false)
                .compact(),
        )
        .init();

    info!("Starting Gitup daemon ...");

    debug!("Initializing storage providers ...");
    let testing_providers = storage_providers::init_providers(&config.providers).map_err(|e| e.to_string())?;
    let mut providers = Arc::new(RwLock::new(Vec::new()));

    debug!("Checking connection to storage providers ...");

    // Limit the number of concurrent tasks
    let semaphore = Arc::new(Semaphore::new(*MAX_THREADS));

    let tasks = stream::iter(testing_providers.into_iter())
        .map(|mut provider| {
            let providers = providers.clone();
            let semaphore = semaphore.clone();

            async move {
                let _permit = semaphore.acquire().await;

                match provider.check_connection().await {
                    Ok(_) => {
                        info!("Connection to provider '{}' successful", provider.name());
                        let mut providers = providers.write().await;
                        providers.push(provider);
                    },
                    Err(e) => {
                        error!("Connection to provider '{}' failed: {}", provider.name(), e);
                        warn!(
                            "Provider {} removed from the active list because of previous error",
                            provider
                        );
                    },
                }
            }
        })
        .buffer_unordered(*MAX_THREADS);

    // Execute the tasks
    tasks.collect::<()>().await;

    // scope restriction, drop the lock after this point
    {
        // Check if there are any providers available
        let working_providers = providers.read().await;
        if working_providers.is_empty() {
            error!("No storage providers available, exiting ...");
            return Err("No storage providers available".to_string());
        }
    }

    let op_mode = config.operational_mode.unwrap();
    if op_mode != OperationalMode::Restore {
        backup(&config, providers)
            .await
            .map_err(|e| e.to_string())?;
    }
    else if op_mode == OperationalMode::Restore {
        todo!("Restore functionality not implemented yet");
    }

    Ok(())
}
