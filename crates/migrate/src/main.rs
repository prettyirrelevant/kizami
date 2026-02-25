//! Standalone migration CLI: Postgres -> fjall.
//!
//! Environment variables:
//! - `DATABASE_URL`: Postgres connection string (required)
//! - `DATA_DIR`: path to fjall data directory (default: ./data)

use std::env;

use tracing_subscriber::EnvFilter;

use kizami_shared::storage::Storage;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    let storage = Storage::open(&data_dir).expect("failed to open fjall storage");

    kizami_migrate::migrate(&database_url, &storage).await;
}
