//! Shared application state for the axum server.
//!
//! Contains the embedded storage handle and the in-memory progress map.
//! The progress map is populated from fjall on startup and updated by ingestion.

use kizami_shared::storage::{ProgressMap, Storage};

/// Shared state passed to all axum handlers via `State<AppState>`.
#[derive(Clone)]
pub struct AppState {
    /// Embedded fjall storage handle for block lookups and cursor reads.
    /// Wraps two keyspaces: `blocks` (keyed by chain_id|timestamp|number) and
    /// `cursors` (keyed by sqd_slug). Thread-safe via internal Arc.
    pub storage: Storage,
    /// In-memory progress map: sqd_slug -> ChainProgress (cursor, head, updated_at).
    /// Populated from fjall on startup, updated by the ingestion loop on every batch.
    /// Head values are ephemeral (not persisted), cursor values mirror fjall state.
    pub progress: ProgressMap,
}
