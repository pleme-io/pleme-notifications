//! Discord webhook client for service lifecycle notifications
//!
//! Sends rich Discord embeds for service startup/failure events.
//! All webhook calls are fire-and-forget to avoid blocking service startup.
//!
//! Wire format matches Shinka's Discord integration for visual consistency.

use reqwest::Client;
use serde::Serialize;
use std::time::Duration;

use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use crate::grafana::GrafanaClient;
use crate::startup::StartupReport;

/// Discord embed colors (decimal RGB values)
pub mod colors {
    pub const SUCCESS: u32 = 0x00D26A;
    pub const FAILURE: u32 = 0xFF4B4B;
    pub const INFO: u32 = 0x5865F2;
    pub const WARNING: u32 = 0xFEE75C;
}

/// Discord webhook message structure
#[derive(Debug, Clone, Serialize)]
pub struct DiscordWebhook {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub embeds: Vec<DiscordEmbed>,
}

/// Discord rich embed structure
#[derive(Debug, Clone, Serialize, Default)]
pub struct DiscordEmbed {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<DiscordField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<DiscordAuthor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<DiscordFooter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Discord embed field
#[derive(Debug, Clone, Serialize)]
pub struct DiscordField {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub inline: bool,
}

/// Discord embed author
#[derive(Debug, Clone, Serialize)]
pub struct DiscordAuthor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Discord embed footer
#[derive(Debug, Clone, Serialize)]
pub struct DiscordFooter {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
}

/// Configuration for the notification client, loaded from env vars
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    pub webhook_url: Option<String>,
    pub username: String,
    pub cluster_name: String,
    pub environment: String,
    pub notify_on_startup: bool,
    pub notify_on_failure: bool,
    pub failure_mention_role: Option<String>,
    pub failure_mention_users: Vec<String>,
}

impl NotificationConfig {
    /// Load from `DISCORD_*` environment variables (same pattern as Shinka)
    pub fn from_env() -> Self {
        let mention_users = std::env::var("DISCORD_FAILURE_MENTION_USERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            webhook_url: std::env::var("DISCORD_WEBHOOK_URL").ok().filter(|s| !s.is_empty()),
            username: std::env::var("DISCORD_USERNAME")
                .unwrap_or_else(|_| "Pleme Deploy".to_string()),
            cluster_name: std::env::var("DISCORD_CLUSTER_NAME")
                .unwrap_or_else(|_| "unknown".to_string()),
            environment: std::env::var("DISCORD_ENVIRONMENT")
                .unwrap_or_else(|_| "unknown".to_string()),
            notify_on_startup: std::env::var("DISCORD_NOTIFY_ON_STARTUP")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            notify_on_failure: std::env::var("DISCORD_NOTIFY_ON_FAILURE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            failure_mention_role: std::env::var("DISCORD_FAILURE_MENTION_ROLE")
                .ok()
                .filter(|s| !s.is_empty()),
            failure_mention_users: mention_users,
        }
    }
}

/// Unified notification client for Discord + Grafana with circuit breaker
#[derive(Clone)]
pub struct NotificationClient {
    service_name: String,
    config: NotificationConfig,
    discord_client: Option<Client>,
    grafana_client: Option<GrafanaClient>,
    circuit_breaker: CircuitBreaker,
}

impl NotificationClient {
    /// Create from environment variables
    pub fn from_env(service_name: &str) -> Self {
        let config = NotificationConfig::from_env();

        let discord_client = config.webhook_url.as_ref().and_then(|_| {
            Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .ok()
        });

        let grafana_client = GrafanaClient::from_env();

        if discord_client.is_some() {
            tracing::info!(
                service = service_name,
                cluster = %config.cluster_name,
                environment = %config.environment,
                "Discord startup notifications enabled"
            );
        }

        if grafana_client.is_some() {
            tracing::info!(
                service = service_name,
                "Grafana annotation posting enabled"
            );
        }

        Self {
            service_name: service_name.to_string(),
            config,
            discord_client,
            grafana_client,
            circuit_breaker: CircuitBreaker::new(
                CircuitBreakerConfig::new("notifications")
                    .with_failure_threshold(3)
                    .with_success_threshold(2)
                    .with_open_timeout(Duration::from_secs(60)),
            ),
        }
    }

    /// Fire-and-forget: send startup success notification (Discord + Grafana)
    pub fn notify_startup_success(&self, report: &StartupReport) {
        if !self.config.notify_on_startup {
            return;
        }

        if let (Some(client), Some(webhook_url)) =
            (self.discord_client.clone(), self.config.webhook_url.clone())
        {
            let embed = self.build_success_embed(report);
            let webhook = DiscordWebhook {
                username: Some(self.config.username.clone()),
                avatar_url: None,
                content: None,
                embeds: vec![embed],
            };
            let cb = self.circuit_breaker.clone();

            tokio::spawn(async move {
                if !cb.should_allow().await {
                    tracing::debug!("Discord notification skipped (circuit breaker open)");
                    return;
                }

                match client.post(&webhook_url).json(&webhook).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        cb.record_success().await;
                        tracing::debug!("Startup success notification sent to Discord");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        cb.record_failure().await;
                        tracing::warn!(status = %status, "Discord webhook returned non-success");
                    }
                    Err(e) => {
                        cb.record_failure().await;
                        tracing::warn!(error = %e, "Failed to send Discord notification");
                    }
                }
            });
        }

        // Grafana annotation (parallel, fire-and-forget)
        if let Some(grafana) = self.grafana_client.clone() {
            let tags = vec![
                format!("service:{}", report.service_name),
                format!("environment:{}", report.environment),
                format!("cluster:{}", report.cluster_name),
                "event:startup_success".to_string(),
            ];
            let text = format!(
                "{} started on {} ({})",
                report.service_name, report.pod_identity.pod_name, report.image_tag
            );
            tokio::spawn(async move {
                grafana.post_annotation(&text, tags).await;
            });
        }
    }

    /// Fire-and-forget: send startup failure notification (Discord + Grafana)
    pub fn notify_startup_failure(&self, report: &StartupReport, error: &str) {
        if !self.config.notify_on_failure {
            return;
        }

        if let (Some(client), Some(webhook_url)) =
            (self.discord_client.clone(), self.config.webhook_url.clone())
        {
            let embed = self.build_failure_embed(report, error);

            // Build mention content
            let content = {
                let mut mentions = Vec::new();
                if let Some(role_id) = &self.config.failure_mention_role {
                    mentions.push(format!("<@&{}>", role_id));
                }
                for user_id in &self.config.failure_mention_users {
                    mentions.push(format!("<@{}>", user_id));
                }
                if mentions.is_empty() {
                    None
                } else {
                    Some(mentions.join(" "))
                }
            };

            let webhook = DiscordWebhook {
                username: Some(self.config.username.clone()),
                avatar_url: None,
                content,
                embeds: vec![embed],
            };
            let cb = self.circuit_breaker.clone();

            tokio::spawn(async move {
                if !cb.should_allow().await {
                    tracing::debug!("Discord failure notification skipped (circuit breaker open)");
                    return;
                }

                match client.post(&webhook_url).json(&webhook).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        cb.record_success().await;
                        tracing::debug!("Startup failure notification sent to Discord");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        cb.record_failure().await;
                        tracing::warn!(status = %status, "Discord webhook returned non-success");
                    }
                    Err(e) => {
                        cb.record_failure().await;
                        tracing::warn!(error = %e, "Failed to send Discord failure notification");
                    }
                }
            });
        }

        // Grafana annotation (parallel, fire-and-forget)
        if let Some(grafana) = self.grafana_client.clone() {
            let tags = vec![
                format!("service:{}", report.service_name),
                format!("environment:{}", report.environment),
                format!("cluster:{}", report.cluster_name),
                "event:startup_failure".to_string(),
            ];
            let text = format!(
                "{} FAILED on {} ({}): {}",
                report.service_name, report.pod_identity.pod_name, report.image_tag, error
            );
            tokio::spawn(async move {
                grafana.post_annotation(&text, tags).await;
            });
        }
    }

    /// Build Discord embed for successful startup
    fn build_success_embed(&self, report: &StartupReport) -> DiscordEmbed {
        let mut fields = vec![
            DiscordField {
                name: "Pod".to_string(),
                value: format!("`{}`", report.pod_identity.pod_name),
                inline: true,
            },
            DiscordField {
                name: "Namespace".to_string(),
                value: format!("`{}`", report.pod_identity.pod_namespace),
                inline: true,
            },
            DiscordField {
                name: "Node".to_string(),
                value: format!("`{}`", report.pod_identity.node_name),
                inline: true,
            },
            DiscordField {
                name: "Image Tag".to_string(),
                value: format!("`{}`", truncate_tag(&report.image_tag, 40)),
                inline: true,
            },
            DiscordField {
                name: "Run Mode".to_string(),
                value: format!("`{}`", report.run_mode),
                inline: true,
            },
            DiscordField {
                name: "Version".to_string(),
                value: format!("`{}`", truncate_tag(&report.git_sha, 12)),
                inline: true,
            },
            DiscordField {
                name: "Startup Duration".to_string(),
                value: format_duration(report.total_duration),
                inline: true,
            },
        ];

        // Dependency status
        let dep_status = &report.dependency_status;
        let mut dep_parts = Vec::new();
        if let Some(db) = &dep_status.database {
            dep_parts.push(format!("Database {} ({}ms)", if db.connected { "\u{2705}" } else { "\u{274C}" }, db.latency.as_millis()));
        }
        if let Some(redis) = &dep_status.redis {
            dep_parts.push(format!("Redis {} ({}ms)", if redis.connected { "\u{2705}" } else { "\u{274C}" }, redis.latency.as_millis()));
        }
        if let Some(nats) = &dep_status.nats {
            dep_parts.push(format!("NATS {} ({}ms)", if nats.connected { "\u{2705}" } else { "\u{274C}" }, nats.latency.as_millis()));
        }
        if !dep_parts.is_empty() {
            fields.push(DiscordField {
                name: "Dependencies".to_string(),
                value: dep_parts.join(", "),
                inline: false,
            });
        }

        // Phases code block
        if !report.phases.is_empty() {
            fields.push(DiscordField {
                name: "Phases".to_string(),
                value: report.phases_code_block(),
                inline: false,
            });
        }

        DiscordEmbed {
            title: Some("\u{1F7E2} Service Started".to_string()),
            description: None,
            color: Some(colors::SUCCESS),
            fields,
            author: Some(DiscordAuthor {
                name: format!(
                    "{} | {} | {}",
                    capitalize_service(&self.service_name),
                    self.config.cluster_name,
                    self.config.environment
                ),
                icon_url: None,
            }),
            footer: Some(DiscordFooter {
                text: format!("{} / {}", self.config.cluster_name, self.config.environment),
                icon_url: None,
            }),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
        }
    }

    /// Build Discord embed for failed startup
    fn build_failure_embed(&self, report: &StartupReport, error: &str) -> DiscordEmbed {
        let error_display = if error.len() > 500 {
            format!("```\n{}...\n```", &error[..497])
        } else {
            format!("```\n{}\n```", error)
        };

        let mut fields = vec![
            DiscordField {
                name: "Pod".to_string(),
                value: format!("`{}`", report.pod_identity.pod_name),
                inline: true,
            },
            DiscordField {
                name: "Namespace".to_string(),
                value: format!("`{}`", report.pod_identity.pod_namespace),
                inline: true,
            },
            DiscordField {
                name: "Image Tag".to_string(),
                value: format!("`{}`", truncate_tag(&report.image_tag, 40)),
                inline: true,
            },
            DiscordField {
                name: "Duration".to_string(),
                value: format_duration(report.total_duration),
                inline: true,
            },
        ];

        // Find the failed phase
        for phase in &report.phases {
            if let crate::startup::PhaseStatus::Failed(ref e) = phase.status {
                fields.push(DiscordField {
                    name: "Failed Phase".to_string(),
                    value: format!("`{}` - {}", phase.name, e),
                    inline: false,
                });
                break;
            }
        }

        // Debug commands
        fields.push(DiscordField {
            name: "Debug Commands".to_string(),
            value: format!(
                "```\nkubectl logs {} -n {} --tail=100\nkubectl describe pod {} -n {}\n```",
                report.pod_identity.pod_name,
                report.pod_identity.pod_namespace,
                report.pod_identity.pod_name,
                report.pod_identity.pod_namespace,
            ),
            inline: false,
        });

        DiscordEmbed {
            title: Some("\u{274C} Service Startup Failed".to_string()),
            description: Some(error_display),
            color: Some(colors::FAILURE),
            fields,
            author: Some(DiscordAuthor {
                name: format!(
                    "{} | {} | {}",
                    capitalize_service(&self.service_name),
                    self.config.cluster_name,
                    self.config.environment
                ),
                icon_url: None,
            }),
            footer: Some(DiscordFooter {
                text: format!("{} / {}", self.config.cluster_name, self.config.environment),
                icon_url: None,
            }),
            timestamp: Some(chrono::Utc::now().to_rfc3339()),
        }
    }
}

/// Capitalize service name for display (e.g., "lilitu-backend" -> "Lilitu Backend")
fn capitalize_service(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Truncate a tag to max length with ellipsis
fn truncate_tag(tag: &str, max_len: usize) -> String {
    if tag.len() <= max_len {
        tag.to_string()
    } else {
        format!("{}...", &tag[..max_len.saturating_sub(3)])
    }
}

/// Format duration in human-readable format
fn format_duration(duration: Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1000 {
        format!("`{}ms`", ms)
    } else {
        format!("`{:.1}s`", duration.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_tag() {
        assert_eq!(truncate_tag("short", 10), "short");
        assert_eq!(truncate_tag("sha256:abc123def456", 10), "sha256:...");
    }

    #[test]
    fn test_capitalize_service() {
        assert_eq!(capitalize_service("lilitu-backend"), "Lilitu Backend");
        assert_eq!(capitalize_service("hanabi"), "Hanabi");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_millis(42)), "`42ms`");
        assert_eq!(format_duration(Duration::from_millis(1200)), "`1.2s`");
    }
}
