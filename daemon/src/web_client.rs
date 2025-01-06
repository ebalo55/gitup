use reqwest::RequestBuilder;
use serde::Deserialize;

/// Append headers to a request builder to make a request to the GitHub API
///
/// # Arguments
///
/// * `req` - The request builder to append headers to
/// * `pat` - The personal access token to authenticate the request
///
/// # Returns
///
/// The request builder with the headers appended
fn prepare_github_request(req: RequestBuilder, pat: &str) -> RequestBuilder {
    req.header("Accept", "application/vnd.github+json")
        .header("User-Agent", "Gitup daemon")
        .header("Authorization", format!("Bearer {}", pat))
        .header("X-GitHub-Api-Version", "2022-11-28")
}

/// A struct representing a GitHub repository
#[derive(Debug, Clone, Eq, PartialEq)]
struct Repository {
    /// The owner of the repository
    owner: String,
    /// The name of the repository
    name: String,
}
impl TryFrom<String> for Repository {
    type Error = RequestError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Check if the value is a GitHub URL
        if value.contains("github.com") {
            let value = value.trim_end_matches(".git");
            let path_start = value.find("github.com/").unwrap() + "github.com/".len();
            let value = &value[path_start..];
            let parts = value.split('/').collect::<Vec<&str>>();

            if parts.len() != 2 {
                return Err(RequestError::InvalidRepositoryUrl);
            }

            return Ok(Self {
                owner: parts[0].to_owned(),
                name: parts[1].to_owned(),
            });
        } else if value.contains("gitlab.com") {
            todo!("GitLab support is not implemented yet")
        }

        Err(RequestError::InvalidRepositoryUrl)
    }
}

/// An enum representing the different endpoints that can be requested from the GitHub API
pub enum RequestEndpoint {
    /// The meta endpoint
    Meta,
}

/// An enum representing the different errors that can occur when making a request
#[derive(Debug)]
pub enum RequestError {
    /// A request error
    RequestError(reqwest::Error),
    /// An invalid repository URL have been provided
    InvalidRepositoryUrl,
}

/// A struct representing useful metadata about a repository (the only one intended to be used)
#[derive(Debug, Deserialize)]
pub struct UsefulMetadata {
    /// Whether the repository is archived
    pub archived: bool,
    /// Whether the repository is disabled
    pub disabled: bool,
    /// The repository visibility
    pub visibility: String,
}

/// Make a request to the GitHub API
pub fn make_request(
    endpoint: RequestEndpoint,
    repository_url: &str,
    pat: &str,
) -> Result<RequestBuilder, RequestError> {
    let client = reqwest::Client::new();
    let repository = Repository::try_from(repository_url.to_owned())?;

    let req = match endpoint {
        RequestEndpoint::Meta => client.get(format!(
            "https://api.github.com/repos/{}/{}",
            repository.owner, repository.name
        )),
    };

    Ok(prepare_github_request(req, pat))
}
