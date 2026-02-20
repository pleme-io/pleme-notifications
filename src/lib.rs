//! Shared notification infrastructure for Pleme services.
//!
//! Provides Discord webhook notifications, Grafana annotations, and startup
//! reporting for service lifecycle events. Extracted from Shinka's battle-tested
//! notification system for reuse across all Pleme services.
//!
//! # Features
//!
//! - **Discord**: Rich embed notifications with circuit breaker protection
//! - **Grafana**: Annotation posting for deployment events
//! - **Startup Reports**: Structured service startup telemetry
//! - **Health Probes**: Dependency verification (feature-gated)

pub mod circuit_breaker;
pub mod discord;
pub mod grafana;
pub mod health_probes;
pub mod startup;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
pub use discord::{NotificationClient, NotificationConfig};
pub use grafana::GrafanaClient;
pub use startup::{
    DependencyCheck, DependencyStatus, PodIdentity, StartupPhase, StartupReport, PhaseStatus,
};
