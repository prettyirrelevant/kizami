//! Kizami API server.
//!
//! Block-by-timestamp lookup API for EVM chains. Serves cached lookups from Postgres
//! and runs a background ingestion loop that fetches block headers from SQD Portal.
//!
//! Environment variables:
//! - `DATABASE_URL`: Postgres connection string (required)
//! - `PORT`: HTTP listen port (default: 8080)
//! - `RUST_LOG`: tracing env filter (default: info)
//! - `INGEST_INTERVAL_SECS`: seconds between ingestion cycles (default: 60)

mod routes;
mod state;

use std::env;

use axum::http::Method;
use axum::routing::get;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use kizami_shared::db;
use kizami_shared::sqd::SqdClient;

use crate::state::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Kizami API",
        description = "Block-by-timestamp lookup API for EVM chains",
        version = "1.0.0"
    ),
    tags(
        (name = "Chains", description = "Chain information endpoints"),
        (name = "Blocks", description = "Block lookup endpoints")
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

    let database_url =
        env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let pool = db::create_pool(&database_url)
        .await
        .expect("failed to create database pool");

    db::run_migrations(&pool)
        .await
        .expect("failed to run migrations");

    tracing::info!("database connected and migrations applied");

    let state = AppState::new(pool.clone());

    // graceful shutdown: ctrl-c signals both the server and ingestion loop
    let shutdown = tokio::signal::ctrl_c();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // spawn ingestion as a background task in the same process
    let sqd_client = SqdClient::new();
    let ingestion_pool = pool.clone();
    let ingestion_cursor_cache = state.cursor_cache.clone();
    tokio::spawn(async move {
        kizami_ingestion::run_ingestion_loop(
            ingestion_pool,
            sqd_client,
            ingestion_cursor_cache,
            shutdown_rx,
        )
        .await;
    });

    let cors = CorsLayer::new()
        .allow_methods([Method::GET])
        .allow_origin(Any);

    let (router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .route("/v1/chains", get(routes::chains::list_chains))
        .route("/v1/chains/{chain_id}", get(routes::chains::get_chain))
        .route(
            "/v1/chains/{chain_id}/block/{direction}/{timestamp}",
            get(routes::blocks::find_block),
        )
        .with_state(state)
        .split_for_parts();

    let app = router
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", api))
        .route("/health", get(|| async { "ok" }))
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
