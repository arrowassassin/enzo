//! Enzo daemon entry point.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "enzo_daemon=info".into()),
        )
        .init();

    let sock_path = enzo_daemon::sock_path_from_env_or_default();
    enzo_daemon::bind_and_serve(&sock_path).await
}
