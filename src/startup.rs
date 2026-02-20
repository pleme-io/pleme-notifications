//! Startup report data structures
//!
//! Captures structured information about service startup for notifications
//! and observability. Designed to work with both Discord embeds and
//! structured logging (Loki).

use std::time::Duration;

/// Pod identity loaded from Kubernetes Downward API environment variables
#[derive(Debug, Clone, Default)]
pub struct PodIdentity {
    pub pod_name: String,
    pub pod_namespace: String,
    pub node_name: String,
}

impl PodIdentity {
    /// Load pod identity from environment variables.
    /// Falls back to sensible defaults for local development.
    pub fn from_env() -> Self {
        Self {
            pod_name: std::env::var("POD_NAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "local".to_string()),
            pod_namespace: std::env::var("POD_NAMESPACE")
                .unwrap_or_else(|_| "local".to_string()),
            node_name: std::env::var("NODE_NAME")
                .unwrap_or_else(|_| "local".to_string()),
        }
    }
}

/// Status of a startup phase
#[derive(Debug, Clone)]
pub enum PhaseStatus {
    Success,
    Failed(String),
    Degraded(String),
}

impl std::fmt::Display for PhaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhaseStatus::Success => write!(f, "ok"),
            PhaseStatus::Failed(e) => write!(f, "failed: {}", e),
            PhaseStatus::Degraded(r) => write!(f, "degraded: {}", r),
        }
    }
}

/// A single startup phase with timing
#[derive(Debug, Clone)]
pub struct StartupPhase {
    pub name: String,
    pub duration: Duration,
    pub status: PhaseStatus,
    pub detail: Option<String>,
}

/// Result of checking a single dependency
#[derive(Debug, Clone)]
pub struct DependencyCheck {
    pub name: String,
    pub connected: bool,
    pub latency: Duration,
    pub detail: Option<String>,
}

impl DependencyCheck {
    /// Create a successful check
    pub fn ok(name: &str, latency: Duration) -> Self {
        Self {
            name: name.to_string(),
            connected: true,
            latency,
            detail: None,
        }
    }

    /// Create a successful check with detail
    pub fn ok_with_detail(name: &str, latency: Duration, detail: String) -> Self {
        Self {
            name: name.to_string(),
            connected: true,
            latency,
            detail: Some(detail),
        }
    }

    /// Create a failed check
    pub fn failed(name: &str, latency: Duration, error: String) -> Self {
        Self {
            name: name.to_string(),
            connected: false,
            latency,
            detail: Some(error),
        }
    }

    /// Create a skipped check (dependency not configured)
    pub fn skipped(name: &str) -> Self {
        Self {
            name: name.to_string(),
            connected: true,
            latency: Duration::ZERO,
            detail: Some("not configured".to_string()),
        }
    }

    /// Format as status string for Discord embed
    pub fn status_string(&self) -> String {
        if self.connected {
            format!("\u{2705} OK ({}ms)", self.latency.as_millis())
        } else {
            format!(
                "\u{274C} FAILED{}",
                self.detail
                    .as_ref()
                    .map(|d| format!(" - {}", d))
                    .unwrap_or_default()
            )
        }
    }
}

/// Aggregated dependency status
#[derive(Debug, Clone, Default)]
pub struct DependencyStatus {
    pub database: Option<DependencyCheck>,
    pub redis: Option<DependencyCheck>,
    pub nats: Option<DependencyCheck>,
}

impl DependencyStatus {
    /// Check if all configured dependencies are healthy
    pub fn all_healthy(&self) -> bool {
        let checks = [&self.database, &self.redis, &self.nats];
        checks
            .iter()
            .filter_map(|c| c.as_ref())
            .all(|c| c.connected)
    }
}

/// Complete startup report for notification
#[derive(Debug, Clone)]
pub struct StartupReport {
    pub service_name: String,
    pub image_tag: String,
    pub pod_identity: PodIdentity,
    pub cluster_name: String,
    pub environment: String,
    pub total_duration: Duration,
    pub phases: Vec<StartupPhase>,
    pub dependency_status: DependencyStatus,
    pub version: String,
    pub git_sha: String,
    pub run_mode: String,
}

impl StartupReport {
    /// Format phases as a code block for Discord
    pub fn phases_code_block(&self) -> String {
        let mut lines = Vec::new();
        for phase in &self.phases {
            let duration_str = if phase.duration.as_millis() < 1000 {
                format!("{}ms", phase.duration.as_millis())
            } else {
                format!("{:.1}s", phase.duration.as_secs_f64())
            };
            lines.push(format!("  {:<14} {}", format!("{}:", phase.name), duration_str));
        }
        format!("```\n{}\n```", lines.join("\n"))
    }
}
