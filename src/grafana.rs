//! Grafana annotation client
//!
//! Posts annotations to Grafana for deployment lifecycle events.
//! Uses the `/api/annotations` endpoint with Bearer token authentication.

use reqwest::Client;
use serde::Serialize;
use std::time::Duration;

/// Grafana annotation client
#[derive(Clone)]
pub struct GrafanaClient {
    client: Client,
    base_url: String,
    api_key: String,
}

#[derive(Serialize)]
struct GrafanaAnnotation {
    text: String,
    tags: Vec<String>,
    time: i64,
}

impl GrafanaClient {
    /// Create from explicit parameters
    pub fn new(base_url: String, api_key: String) -> Option<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .ok()?;

        Some(Self {
            client,
            base_url,
            api_key,
        })
    }

    /// Create from `GRAFANA_URL` and `GRAFANA_API_KEY` environment variables
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("GRAFANA_URL").ok()?;
        let api_key = std::env::var("GRAFANA_API_KEY").ok()?;

        if base_url.is_empty() || api_key.is_empty() {
            return None;
        }

        Self::new(base_url, api_key)
    }

    /// Post an annotation to Grafana
    pub async fn post_annotation(&self, text: &str, tags: Vec<String>) {
        let annotation = GrafanaAnnotation {
            text: text.to_string(),
            tags,
            time: chrono::Utc::now().timestamp_millis(),
        };

        let url = format!("{}/api/annotations", self.base_url.trim_end_matches('/'));

        match self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&annotation)
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("Grafana annotation posted successfully");
            }
            Ok(resp) => {
                tracing::warn!(
                    status = %resp.status(),
                    "Grafana annotation POST returned non-success"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to post Grafana annotation");
            }
        }
    }
}
