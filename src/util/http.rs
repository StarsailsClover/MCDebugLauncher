// HTTP utilities

use anyhow::{Context, Result};
use reqwest::Client;
use std::time::Duration;

/// Create a configured HTTP client
pub fn create_http_client() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(format!("MDL/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to create HTTP client")
}

/// Fetch JSON from URL
pub async fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let client = create_http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .context(format!("Failed to fetch {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP error {}: {}", response.status(), url);
    }

    response
        .json::<T>()
        .await
        .context("Failed to parse JSON response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_http_client() {
        let client = create_http_client();
        assert!(client.is_ok());
    }
}
