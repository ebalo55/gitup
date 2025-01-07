mod configuration;
mod storage_providers;
mod web_client;

use std::{fs, path::PathBuf};

use clap::{Parser, ValueEnum};
use optional_struct::{optional_struct, Applicable};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, field::debug, info, warn, Level};
use tracing_subscriber::layer::SubscriberExt;

use crate::{
    configuration::{Args, OptionalArgs},
    web_client::{make_request, RequestEndpoint, UsefulMetadata},
};

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = Args::parse();

    // Setup tracing
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_ansi(true)
            .with_level(true)
            .with_file(false)
            .with_line_number(false)
            .compact()
            .finish()
            .with(tracing_subscriber::filter::filter_fn(move |metadata| {
                match metadata.level() {
                    &Level::TRACE => false,
                    &Level::DEBUG => args.debug,
                    _ => true,
                }
            })),
    )
    .unwrap();

    let config = if args.config.is_some() && args.config.as_ref().unwrap().is_file() {
        let config_ref = args.config.as_ref().unwrap();
        debug!("Configuration file found, loading ...");
        info!("Loading configuration from '{}'", config_ref.display());

        // Load configuration from file
        let config = fs::read_to_string(config_ref.as_path());

        if config.is_err() {
            error!(
                "Failed to read configuration file: {}",
                config.as_ref().err().unwrap()
            );
            return Err(config.err().unwrap().to_string());
        }

        debug!("Parsing configuration ...");
        let config = serde_json::from_str::<OptionalArgs>(config.unwrap().as_str());
        if config.is_err() {
            error!(
                "Failed to parse configuration file: {}",
                config.as_ref().err().unwrap()
            );
            return Err(config.err().unwrap().to_string());
        }
        let config = config.unwrap();

        debug!("Merging configuration ...");
        // Merge configuration from file with command line arguments
        let config = config.build(args);

        info!("Configuration loaded successfully");

        config
    }
    else {
        info!("No configuration file found, using command line arguments");

        args
    };

    info!("Starting Gitup daemon ...");

    debug!("Checking connection to repository ...");
    check_connection(&config).await?;
    debug!("Connection to repository successful");

    Ok(())
}

/// Check the connection to the repository
async fn check_connection(args: &Args) -> Result<(), String> {
    let repository_url = args.repository_url.as_ref().unwrap();
    let pat = args.pat.as_ref().unwrap();

    // prepare the request
    let req = make_request(RequestEndpoint::Meta, repository_url, pat);
    if req.is_err() {
        error!("Failed to create request: {}", req.as_ref().err().unwrap());
        return Err(req.err().unwrap().to_string());
    }
    let req = req.unwrap();
    let response = req.send().await;

    if response.is_err() {
        error!(
            "Failed to connect to repository: {}",
            response.as_ref().err().unwrap()
        );
        return Err(response.err().unwrap().to_string());
    }
    let response = response.unwrap();

    if response.status().is_success() {
        let response = response.json::<UsefulMetadata>().await;

        if response.is_err() {
            error!(
                "Failed to parse response: {}",
                response.as_ref().err().unwrap()
            );
            return Err(response.err().unwrap().to_string());
        }
        let response = response.unwrap();

        if response.archived {
            error!("Repository archived, cannot continue");
            return Err("Repository archived, cannot continue".to_string());
        }
        if response.disabled {
            error!("Repository disabled, cannot continue");
            return Err("Repository disabled, cannot continue".to_string());
        }

        debug!("Repository visibility: {}", response.visibility);

        if response.visibility != "private" && response.visibility != "internal" {
            warn!("Repository is public, consider making it private");
        }

        return Ok(());
    }

    error!("Failed to connect to repository: {}", response.status());
    Err(format!(
        "Request responded with status {}",
        response.status().to_string()
    ));
}
