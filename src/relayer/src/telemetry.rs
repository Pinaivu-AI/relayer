//! Tracing setup.

use tracing_subscriber::EnvFilter;

pub fn init() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("chat_relayer=info,tower_http=info")),
        )
        .init();
}
