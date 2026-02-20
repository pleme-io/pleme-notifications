//! Circuit Breaker pattern implementation
//!
//! Provides protection against cascading failures when external services
//! (Discord, Grafana, etc.) are unavailable.
//!
//! Extracted from Shinka's production circuit breaker.
//!
//! ## States
//!
//! - **Closed**: Normal operation, requests are allowed
//! - **Open**: Circuit is tripped, requests are rejected immediately
//! - **HalfOpen**: Testing if service has recovered
//!
//! ## Configuration
//!
//! - `failure_threshold`: Number of failures before opening circuit (default: 3)
//! - `success_threshold`: Number of successes to close circuit (default: 2)
//! - `timeout`: Time to wait before transitioning from Open to HalfOpen (default: 60s)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests allowed
    Closed,
    /// Circuit tripped - requests rejected
    Open,
    /// Testing recovery - limited requests allowed
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half_open"),
        }
    }
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Name for logging/metrics
    pub name: String,
    /// Number of consecutive failures to trip the circuit
    pub failure_threshold: u32,
    /// Number of consecutive successes to close the circuit
    pub success_threshold: u32,
    /// Time to wait before testing recovery
    pub open_timeout: Duration,
    /// Maximum requests allowed in half-open state
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            failure_threshold: 3,
            success_threshold: 2,
            open_timeout: Duration::from_secs(60),
            half_open_max_requests: 3,
        }
    }
}

impl CircuitBreakerConfig {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn with_failure_threshold(mut self, threshold: u32) -> Self {
        self.failure_threshold = threshold;
        self
    }

    pub fn with_success_threshold(mut self, threshold: u32) -> Self {
        self.success_threshold = threshold;
        self
    }

    pub fn with_open_timeout(mut self, timeout: Duration) -> Self {
        self.open_timeout = timeout;
        self
    }
}

/// Circuit breaker state (thread-safe)
struct CircuitBreakerState {
    state: CircuitState,
    consecutive_failures: u32,
    consecutive_successes: u32,
    last_failure_time: Option<Instant>,
    half_open_requests: u32,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_failure_time: None,
            half_open_requests: 0,
        }
    }
}

/// Circuit breaker for protecting external service calls
#[derive(Clone)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: Arc<RwLock<CircuitBreakerState>>,
    failures_total: Arc<AtomicU64>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: Arc::new(RwLock::new(CircuitBreakerState::default())),
            failures_total: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Check if a request should be allowed
    pub async fn should_allow(&self) -> bool {
        let mut state = self.state.write().await;
        let current_state = self.evaluate_state(&state);

        match current_state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                tracing::debug!(
                    circuit = %self.config.name,
                    state = %current_state,
                    "Circuit breaker rejected request"
                );
                false
            }
            CircuitState::HalfOpen => {
                if state.half_open_requests < self.config.half_open_max_requests {
                    state.half_open_requests += 1;
                    true
                } else {
                    tracing::debug!(
                        circuit = %self.config.name,
                        state = %current_state,
                        "Circuit breaker rejected request (half-open limit)"
                    );
                    false
                }
            }
        }
    }

    /// Record a successful request
    pub async fn record_success(&self) {
        let mut state = self.state.write().await;
        state.consecutive_failures = 0;
        state.consecutive_successes += 1;

        let current_state = self.evaluate_state(&state);

        if current_state == CircuitState::HalfOpen
            && state.consecutive_successes >= self.config.success_threshold
        {
            tracing::info!(
                circuit = %self.config.name,
                "Circuit breaker closing after successful recovery"
            );
            state.state = CircuitState::Closed;
            state.consecutive_successes = 0;
            state.half_open_requests = 0;
        }
    }

    /// Record a failed request
    pub async fn record_failure(&self) {
        self.failures_total.fetch_add(1, Ordering::Relaxed);

        let mut state = self.state.write().await;
        state.consecutive_successes = 0;
        state.consecutive_failures += 1;
        state.last_failure_time = Some(Instant::now());

        let current_state = self.evaluate_state(&state);

        match current_state {
            CircuitState::Closed => {
                if state.consecutive_failures >= self.config.failure_threshold {
                    tracing::warn!(
                        circuit = %self.config.name,
                        failures = state.consecutive_failures,
                        "Circuit breaker opening due to failures"
                    );
                    state.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                tracing::warn!(
                    circuit = %self.config.name,
                    "Circuit breaker reopening after half-open failure"
                );
                state.state = CircuitState::Open;
                state.half_open_requests = 0;
            }
            CircuitState::Open => {}
        }
    }

    /// Evaluate the current state, potentially transitioning from Open to HalfOpen
    fn evaluate_state(&self, state: &CircuitBreakerState) -> CircuitState {
        if state.state == CircuitState::Open {
            if let Some(last_failure) = state.last_failure_time {
                if last_failure.elapsed() >= self.config.open_timeout {
                    return CircuitState::HalfOpen;
                }
            }
        }
        state.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_closed_by_default() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::new("test"));
        assert!(cb.should_allow().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_failures() {
        let config = CircuitBreakerConfig::new("test").with_failure_threshold(3);
        let cb = CircuitBreaker::new(config);

        for _ in 0..3 {
            cb.record_failure().await;
        }

        assert!(!cb.should_allow().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_closes_after_successes() {
        let config = CircuitBreakerConfig::new("test")
            .with_failure_threshold(2)
            .with_success_threshold(2)
            .with_open_timeout(Duration::from_millis(10));

        let cb = CircuitBreaker::new(config);

        cb.record_failure().await;
        cb.record_failure().await;
        assert!(!cb.should_allow().await);

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(cb.should_allow().await); // half-open allows

        cb.record_success().await;
        cb.record_success().await;
        assert!(cb.should_allow().await); // closed again
    }
}
