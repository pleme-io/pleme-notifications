# pleme-notifications

Shared notification infrastructure for Pleme Rust services. Provides Discord webhook notifications, Grafana annotations, and structured startup reporting with circuit breaker protection.

## Features

- **Discord**: Rich embed notifications for service startup/failure events
- **Grafana**: Annotation posting for deployment lifecycle events
- **Startup Reports**: Structured telemetry for service startup phases
- **Health Probes**: Generic dependency verification (database, Redis, NATS)
- **Circuit Breaker**: Protection against cascading failures to external services

## Usage

```toml
[dependencies]
pleme-notifications = { git = "https://github.com/pleme-io/pleme-notifications" }
```

```rust
use pleme_notifications::{NotificationClient, StartupReport, PodIdentity};

let client = NotificationClient::from_env("my-service");
// ... build startup report ...
client.notify_startup_success(&report);
```

## Configuration

All configuration is via environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `DISCORD_WEBHOOK_URL` | Discord webhook URL | _(disabled)_ |
| `DISCORD_USERNAME` | Bot display name | `Pleme Deploy` |
| `DISCORD_CLUSTER_NAME` | Cluster identifier | `unknown` |
| `DISCORD_ENVIRONMENT` | Environment name | `unknown` |
| `DISCORD_NOTIFY_ON_STARTUP` | Send startup notifications | `true` |
| `DISCORD_NOTIFY_ON_FAILURE` | Send failure notifications | `true` |
| `GRAFANA_URL` | Grafana base URL | _(disabled)_ |
| `GRAFANA_API_KEY` | Grafana API key | _(disabled)_ |

## License

MIT
