use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    sync::Arc,
};

use async_trait::async_trait;
use regex::Regex;
use reqwest::RequestBuilder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, warn};

use crate::{
    base64,
    byte_size::{format_bytesize, GIGABYTE, KILOBYTE, MEGABYTE},
    storage_providers::provider::{CreatableStorageProvider, ProviderError, StorageProvider},
};

static MAX_REPOSITORY_SIZE: u64 = 1 * GIGABYTE;

/// An enum representing the different endpoints that can be requested from the GitHub API
enum RequestEndpoint {
    /// The meta endpoint
    Meta,
    /// Commit endpoint, upload data to the repository
    Commit,
}

/// A struct representing useful metadata about a repository (the only one intended to be used)
#[derive(Debug, Deserialize)]
pub struct UsefulMetadata {
    /// Whether the repository is archived
    pub archived:   bool,
    /// Whether the repository is disabled
    pub disabled:   bool,
    /// The repository visibility
    pub visibility: String,
    /// The repository size in kilobytes
    pub size:       u64,
}

/// A struct representing the data to commit to the repository
#[derive(Debug, Serialize)]
pub struct CommitData {
    /// The commit message
    pub message: String,
    /// The content to commit
    pub content: String,
}

/// GitHub storage provider
#[derive(Debug, Clone, Default)]
pub struct Provider {
    /// The owner of the repository
    owner:           String,
    /// The repository name
    repo:            String,
    /// The personal access token
    pat:             String,
    /// The available space in the repository
    available_space: u64,
}

impl Provider {
    /// Append headers to a request builder to make a request to the GitHub API
    ///
    /// # Arguments
    ///
    /// * `req` - The request builder to append headers to
    ///
    /// # Returns
    ///
    /// The request builder with the headers appended
    fn prepare_github_request_headers(&self, req: RequestBuilder) -> RequestBuilder {
        req.header("Accept", "application/vnd.github+json")
            .header("User-Agent", "Gitup daemon")
            .header("Authorization", format!("Bearer {}", self.pat))
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    /// Prepare a request to the GitHub API
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The endpoint to request
    ///
    /// # Returns
    ///
    /// A request builder to make the request
    fn make_request(&self, endpoint: RequestEndpoint, extra: Option<HashMap<String, String>>) -> RequestBuilder {
        let client = reqwest::Client::new();

        match endpoint {
            RequestEndpoint::Meta => {
                self.prepare_github_request_headers(client.get(format!(
                    "https://api.github.com/repos/{}/{}",
                    self.owner, self.repo
                )))
            },
            RequestEndpoint::Commit => {
                self.prepare_github_request_headers(client.put(format!(
                    "https://api.github.com/repos/{}/{}/contents/{}",
                    self.owner,
                    self.repo,
                    extra.unwrap().get("path").unwrap()
                )))
            },
        }
    }
}

impl Display for Provider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (gitup://****:github/{}/{})",
            self.name(),
            self.owner,
            self.repo
        )
    }
}

#[async_trait]
impl StorageProvider for Provider {
    fn name(&self) -> &'static str { "GitHub" }

    fn url(&self) -> &'static str { "gitup://<auth-token>:github/<owner>/<repo>" }

    async fn check_connection(&mut self) -> Result<(), ProviderError> {
        // prepare the request and send it
        let req = self.make_request(RequestEndpoint::Meta, None);
        let req = req.send().await;

        // check for errors
        if req.is_err() {
            return Err(ProviderError::ConnectionError(
                req.err().unwrap().to_string(),
            ));
        }
        let req = req.unwrap();

        if req.status().is_success() {
            // parse the response body
            let response = req.json::<UsefulMetadata>().await;

            if response.is_err() {
                return Err(ProviderError::ConnectionError(
                    response.err().unwrap().to_string(),
                ));
            }
            let response = response.unwrap();

            // check for preconditions
            if response.archived {
                return Err(ProviderError::PreconditionsFailed(
                    "Repository archived, cannot continue".to_owned(),
                ));
            }
            if response.disabled {
                return Err(ProviderError::PreconditionsFailed(
                    "Repository disabled, cannot continue".to_owned(),
                ));
            }
            if response.visibility.to_lowercase() == "public" {
                warn!("Repository is public, you should strongly consider making it private");
            }

            let git_size = response.size * KILOBYTE;
            if git_size > 750 * MEGABYTE && git_size <= 1 * GIGABYTE {
                warn!("Repository size exceeds 750MB, you're likely to violate GitHub's terms of service soon");
            }
            else if git_size > 1 * GIGABYTE {
                return Err(ProviderError::PreconditionsFailed(
                    "Repository size exceeds 1GB, cannot continue".to_owned(),
                ));
            }

            // update the available space and return
            self.available_space -= git_size;

            return Ok(());
        }

        Err(ProviderError::ConnectionError(format!(
            "Failed to connect to repository: {}",
            req.status()
        )))
    }

    fn clone_box(&self) -> Box<dyn StorageProvider> { Box::new(self.clone()) }

    async fn upload(&self, path: String, data: Arc<Vec<u8>>) -> Result<(), ProviderError> {
        // check if there's enough space in the repository

        // The following check has been delegated to the backup engine, refer to the comment at the end of
        // this method to understand why.
        // if data.len() as u64 > self.available_space {
        // return Err(ProviderError::PreconditionsFailed(
        // "Not enough space in the repository".to_owned(),
        // ));
        // }

        // encode the data in base64 (GitHub requires it)
        let data_size = data.len();
        let data = base64::Encoder::new(base64::Variant::Standard).encode(data.as_slice());

        if data.is_err() {
            return Err(ProviderError::GenericError(data.err().unwrap().to_string()));
        }
        let data = data.unwrap();

        debug!(
            "Uploading {} bytes to GitHub at '{}'",
            format_bytesize(data_size as u64),
            path
        );
        let req = self
            .make_request(
                RequestEndpoint::Commit,
                Some([("path".to_owned(), path)].iter().cloned().collect()),
            )
            .json(&CommitData {
                message: "Gitup backup".to_owned(),
                content: data,
            })
            .send()
            .await;

        // check for errors
        if req.is_err() {
            return Err(ProviderError::ConnectionError(
                req.err().unwrap().to_string(),
            ));
        }
        let req = req.unwrap();

        if !req.status().is_success() {
            let status = req.status();
            debug!(
                "Failed to upload data, the api answered with {:?}",
                req.text().await.unwrap()
            );
            return Err(ProviderError::ConnectionError(format!(
                "Failed to upload data: {}",
                status
            )));
        }

        // update the available space and return

        // The following line was commented out to avoid deadlocks due to the mutable requirement of self.
        // Uncommenting this will result in a drastical reduction of the overall performance of the daemon.
        // self.available_space -= data_size as u64;
        debug!(
            "Uploaded {} bytes to GitHub",
            format_bytesize(data_size as u64)
        );
        Ok(())
    }

    fn get_available_space(&self) -> u64 { self.available_space }
}

impl CreatableStorageProvider for Provider {
    fn is_provider_url(url: &str) -> bool {
        let rex = Regex::new(r"gitup://[^:]+:github/[^/]+/.+").unwrap();
        rex.is_match(url)
    }

    fn new(url: &str) -> Result<Self, ProviderError> {
        let rex = Regex::new(r"gitup://(?P<pat>[^:]+):github/(?P<owner>[^/]+)/(?P<repo>.+)").unwrap();
        let caps = rex.captures(url);

        if caps.is_none() {
            return Err(ProviderError::InvalidProviderUrl);
        }
        let caps = caps.unwrap();

        Ok(Self {
            owner:           caps.name("owner").unwrap().as_str().to_string(),
            repo:            caps.name("repo").unwrap().as_str().to_string(),
            pat:             caps.name("pat").unwrap().as_str().to_string(),
            available_space: MAX_REPOSITORY_SIZE,
        })
    }
}
