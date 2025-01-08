mod backup;
mod base64;
mod byte_size;
mod configuration;
mod storage_providers;

use std::{collections::HashMap, fs, io, mem, path::PathBuf, sync::Arc};

use clap::{Parser, ValueEnum};
use optional_struct::{optional_struct, Applicable};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
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
    configuration::{merge_configuration_file, Args, OptionalArgs},
    storage_providers::provider::StorageProvider,
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = Args::parse();

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
                    if args.debug {
                        Level::DEBUG
                    } else {
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

    let config = merge_configuration_file(args)?;

    info!("Starting Gitup daemon ...");

    debug!("Initializing storage providers ...");
    let testing_providers = storage_providers::init_providers(&config.providers).map_err(|e| e.to_string())?;
    let mut providers = Arc::new(RwLock::new(Vec::new()));

    debug!("Checking connection to storage providers ...");

    let mut pending_provider_checks = Vec::new();
    for (_, mut provider) in testing_providers.into_iter().enumerate() {
        let providers = providers.clone();
        pending_provider_checks.push(tokio::spawn(async move {
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
        }));
    }
    // Wait for all provider checks to finish
    futures::future::join_all(pending_provider_checks).await;

    // scope restriction, drop the lock after this point
    {
        // Check if there are any providers available
        let working_providers = providers.read().await;
        if working_providers.is_empty() {
            error!("No storage providers available, exiting ...");
            return Err("No storage providers available".to_string());
        }
    }

    backup(&config, providers)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}
