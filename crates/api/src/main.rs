//! Kizami API server.
//!
//! Block-by-timestamp lookup API for EVM chains. Serves lookups from embedded fjall storage
//! and runs a background ingestion loop that fetches block headers from SQD Portal.
//!
//! Environment variables:
//! - `DATA_DIR`: path to fjall data directory (default: ./data)
//! - `PORT`: HTTP listen port (default: 8080)
//! - `RUST_LOG`: tracing env filter (default: info)
//! - `INGEST_INTERVAL_SECS`: seconds between ingestion cycles (default: 60)
//! - `DATABASE_URL`: if set, runs a one-time Postgres -> fjall migration on startup

mod routes;
mod state;

use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use axum::http::{header, Method};
use axum::routing::get;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use utoipa_swagger_ui::SwaggerUi;

use kizami_shared::sqd::SqdClient;
use kizami_shared::storage::{ChainProgress, Storage};

use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Kizami API",
        description = "Block-by-timestamp lookup API for EVM chains",
        version = "1.0.0",
        license(name = "MIT")
    ),
    tags(
        (name = "Chains", description = "Chain information endpoints"),
        (name = "Blocks", description = "Block lookup endpoints"),
        (name = "Status", description = "Indexing status endpoints")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let storage = Storage::open(&data_dir).expect("failed to open storage");

    tracing::info!(data_dir = %data_dir, "storage opened");

    // one-time postgres migration if DATABASE_URL is set
    if let Ok(database_url) = env::var("DATABASE_URL") {
        kizami_migrate::migrate(&database_url, &storage).await;
    }

    // populate progress map from persisted cursors
    let cursors = storage
        .get_all_cursors()
        .expect("failed to read cursors from storage");
    let mut map = HashMap::new();
    for (slug, last_block, updated_at) in cursors {
        map.insert(
            slug,
            ChainProgress {
                cursor: last_block,
                head: None,
                updated_at: Some(updated_at),
            },
        );
    }
    let progress = Arc::new(RwLock::new(map));

    let state = AppState {
        storage: storage.clone(),
        progress: progress.clone(),
    };

    // graceful shutdown: ctrl-c signals both the server and ingestion loop
    let shutdown = tokio::signal::ctrl_c();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // spawn ingestion as a background task in the same process
    let sqd_client = SqdClient::new();
    tokio::spawn(async move {
        kizami_ingestion::run_ingestion_loop(storage, sqd_client, progress, shutdown_rx).await;
    });

    let cors = CorsLayer::new()
        .allow_methods([Method::GET])
        .allow_origin(Any);

    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(routes::chains::list_chains))
        .routes(routes!(routes::chains::get_chain))
        .routes(routes!(routes::blocks::find_block))
        .routes(routes!(routes::status::indexing_status))
        .with_state(state)
        .split_for_parts();

    let app = router
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", api))
        .route("/health", get(|| async { "ok" }))
        .route(
            "/",
            get(|| async { axum::response::Html(include_str!("../../../static/index.html")) }),
        )
        .route(
            "/static/chains/143.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../../../static/chains/143.svg"),
                )
            }),
        )
        .route(
            "/static/chains/1116.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../../../static/chains/1116.svg"),
                )
            }),
        )
        .route(
            "/static/chains/4200.webp",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/webp")],
                    include_bytes!("../../../static/chains/4200.webp").as_slice(),
                )
            }),
        )
        .layer(cors);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind");

    tracing::info!(port = %port, "server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
            let _ = shutdown_tx.send(());
            tracing::info!("shutdown signal received");
        })
        .await
        .expect("server error");
}
