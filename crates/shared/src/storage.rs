use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use fjall::{Database, Keyspace, KeyspaceCreateOptions, PersistMode};
use tokio::sync::RwLock;

use crate::error::AppError;

/// Progress tracking for a single chain's ingestion state.
#[derive(Debug, Clone)]
pub struct ChainProgress {
    /// Last persisted cursor (ingested block number).
    pub cursor: i64,
    /// Ephemeral tip block number from SQD (not persisted).
    pub head: Option<i64>,
    /// When the cursor was last updated.
    pub updated_at: Option<DateTime<Utc>>,
}

/// Shared progress map: sqd_slug -> ChainProgress.
pub type ProgressMap = Arc<RwLock<HashMap<String, ChainProgress>>>;

/// Embedded storage backed by fjall (LSM-tree key-value store).
///
/// Two keyspaces:
/// - `blocks`: key = `chain_id(4B) | timestamp(8B) | number(8B)`, value = empty
/// - `cursors`: key = sqd_slug (UTF-8), value = `last_block(8B) | updated_at_secs(8B)`
#[derive(Clone)]
pub struct Storage {
    db: Database,
    blocks: Keyspace,
    cursors: Keyspace,
}

// key layout constants
const CHAIN_ID_LEN: usize = 4;
const TIMESTAMP_LEN: usize = 8;
const NUMBER_LEN: usize = 8;
const BLOCK_KEY_LEN: usize = CHAIN_ID_LEN + TIMESTAMP_LEN + NUMBER_LEN;

fn encode_block_key(chain_id: u32, timestamp: u64, number: u64) -> [u8; BLOCK_KEY_LEN] {
    let mut key = [0u8; BLOCK_KEY_LEN];
    key[..CHAIN_ID_LEN].copy_from_slice(&chain_id.to_be_bytes());
    key[CHAIN_ID_LEN..CHAIN_ID_LEN + TIMESTAMP_LEN].copy_from_slice(&timestamp.to_be_bytes());
    key[CHAIN_ID_LEN + TIMESTAMP_LEN..].copy_from_slice(&number.to_be_bytes());
    key
}

fn decode_block_key(key: &[u8]) -> (u32, u64, u64) {
    let chain_id = u32::from_be_bytes(key[..CHAIN_ID_LEN].try_into().unwrap());
    let timestamp = u64::from_be_bytes(
        key[CHAIN_ID_LEN..CHAIN_ID_LEN + TIMESTAMP_LEN]
            .try_into()
            .unwrap(),
    );
    let number = u64::from_be_bytes(key[CHAIN_ID_LEN + TIMESTAMP_LEN..].try_into().unwrap());
    (chain_id, timestamp, number)
}

/// Encode cursor value: last_block (8B i64 BE) | updated_at unix secs (8B i64 BE).
fn encode_cursor_value(last_block: i64, updated_at_secs: i64) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&last_block.to_be_bytes());
    buf[8..].copy_from_slice(&updated_at_secs.to_be_bytes());
    buf
}

fn decode_cursor_value(val: &[u8]) -> (i64, i64) {
    let last_block = i64::from_be_bytes(val[..8].try_into().unwrap());
    let updated_at_secs = i64::from_be_bytes(val[8..].try_into().unwrap());
    (last_block, updated_at_secs)
}

impl Storage {
    /// Opens (or creates) persistent storage at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let db = Database::builder(path)
            .cache_size(64 * 1024 * 1024)
            .open()?;
        let blocks = db.keyspace("blocks", KeyspaceCreateOptions::default)?;
        let cursors = db.keyspace("cursors", KeyspaceCreateOptions::default)?;
        Ok(Self {
            db,
            blocks,
            cursors,
        })
    }

    /// Finds the closest block to a given timestamp in the specified direction.
    ///
    /// Returns `(number, timestamp)` or `None`.
    pub fn find_block(
        &self,
        chain_id: i32,
        timestamp: i64,
        direction: &str,
        inclusive: bool,
    ) -> Result<Option<(i64, i64)>, AppError> {
        let c = chain_id as u32;
        let ts = timestamp as u64;

        let result = match (direction, inclusive) {
            // before inclusive: ts <= T => range(C|0|0 ..= C|T|MAX).next_back()
            ("before", true) => {
                let lo = encode_block_key(c, 0, 0);
                let hi = encode_block_key(c, ts, u64::MAX);
                self.blocks.range(lo..=hi).next_back()
            }
            // before exclusive: ts < T => range(C|0|0 .. C|T|0).next_back()
            ("before", false) => {
                let lo = encode_block_key(c, 0, 0);
                let hi = encode_block_key(c, ts, 0);
                self.blocks.range(lo..hi).next_back()
            }
            // after inclusive: ts >= T => range(C|T|0 .. C+1|0|0).next()
            ("after", true) => {
                let lo = encode_block_key(c, ts, 0);
                let hi = encode_block_key(c + 1, 0, 0);
                self.blocks.range(lo..hi).next()
            }
            // after exclusive: ts > T => range(C|T+1|0 .. C+1|0|0).next()
            ("after", false) => {
                let lo = encode_block_key(c, ts + 1, 0);
                let hi = encode_block_key(c + 1, 0, 0);
                self.blocks.range(lo..hi).next()
            }
            _ => None,
        };

        match result {
            Some(guard) => {
                let key = guard.key()?;
                let (_, block_ts, block_num) = decode_block_key(&key);
                Ok(Some((block_num as i64, block_ts as i64)))
            }
            None => Ok(None),
        }
    }

    /// Bulk-inserts blocks from parallel number/timestamp slices.
    /// Idempotent (overwrites with same empty value).
    pub fn insert_blocks(
        &self,
        chain_id: i32,
        numbers: &[i64],
        timestamps: &[i64],
    ) -> Result<(), AppError> {
        let c = chain_id as u32;
        for (num, ts) in numbers.iter().zip(timestamps.iter()) {
            self.blocks
                .insert(encode_block_key(c, *ts as u64, *num as u64), [])?;
        }
        Ok(())
    }

    /// Bulk-inserts blocks from BlockHeader slice, avoiding intermediate Vec allocations.
    /// Idempotent (overwrites with same empty value).
    pub fn insert_block_headers(
        &self,
        chain_id: i32,
        headers: &[crate::sqd::BlockHeader],
    ) -> Result<(), AppError> {
        let c = chain_id as u32;
        for h in headers {
            self.blocks
                .insert(encode_block_key(c, h.timestamp as u64, h.number as u64), [])?;
        }
        Ok(())
    }

    /// Returns the last ingested block number for a chain, or 0 if no cursor exists.
    pub fn get_cursor(&self, sqd_slug: &str) -> Result<i64, AppError> {
        match self.cursors.get(sqd_slug)? {
            Some(val) => Ok(decode_cursor_value(&val).0),
            None => Ok(0),
        }
    }

    /// Upserts the ingestion cursor for a chain.
    pub fn upsert_cursor(&self, sqd_slug: &str, last_block: i64) -> Result<(), AppError> {
        self.cursors.insert(
            sqd_slug,
            encode_cursor_value(last_block, Utc::now().timestamp()),
        )?;
        Ok(())
    }

    /// Returns all cursors as `(sqd_slug, last_block, updated_at)`.
    pub fn get_all_cursors(&self) -> Result<Vec<(String, i64, DateTime<Utc>)>, AppError> {
        let mut results = Vec::new();
        for guard in self.cursors.iter() {
            let (key, value) = guard.into_inner()?;
            let (last_block, updated_at_secs) = decode_cursor_value(&value);
            if let Some(dt) = DateTime::from_timestamp(updated_at_secs, 0) {
                results.push((
                    String::from_utf8(key.to_vec()).unwrap_or_default(),
                    last_block,
                    dt,
                ));
            }
        }
        Ok(results)
    }

    /// Flushes all data to disk for guaranteed durability.
    pub fn persist(&self) -> Result<(), AppError> {
        self.db.persist(PersistMode::SyncAll)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_storage() -> (Storage, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        (storage, dir)
    }

    #[test]
    fn encode_decode_block_key_roundtrip() {
        let key = encode_block_key(1, 1000, 42);
        let (chain_id, ts, num) = decode_block_key(&key);
        assert_eq!(chain_id, 1);
        assert_eq!(ts, 1000);
        assert_eq!(num, 42);
    }

    #[test]
    fn block_key_lexicographic_ordering() {
        let k1 = encode_block_key(1, 100, 5);
        let k2 = encode_block_key(1, 200, 3);
        let k3 = encode_block_key(2, 50, 1);
        assert!(k1 < k2, "same chain, lower timestamp should sort first");
        assert!(k2 < k3, "lower chain_id should sort before higher");
    }

    #[test]
    fn encode_decode_cursor_value_roundtrip() {
        let val = encode_cursor_value(12345, 1700000000);
        let (block, ts) = decode_cursor_value(&val);
        assert_eq!(block, 12345);
        assert_eq!(ts, 1700000000);
    }

    #[test]
    fn insert_and_find_block_before_inclusive() {
        let (storage, _dir) = test_storage();
        storage
            .insert_blocks(1, &[100, 101, 102], &[1000, 2000, 3000])
            .unwrap();

        let result = storage.find_block(1, 2000, "before", true).unwrap();
        assert_eq!(result, Some((101, 2000)));
    }

    #[test]
    fn insert_and_find_block_before_exclusive() {
        let (storage, _dir) = test_storage();
        storage
            .insert_blocks(1, &[100, 101, 102], &[1000, 2000, 3000])
            .unwrap();

        let result = storage.find_block(1, 2000, "before", false).unwrap();
        assert_eq!(result, Some((100, 1000)));
    }

    #[test]
    fn insert_and_find_block_after_inclusive() {
        let (storage, _dir) = test_storage();
        storage
            .insert_blocks(1, &[100, 101, 102], &[1000, 2000, 3000])
            .unwrap();

        let result = storage.find_block(1, 2000, "after", true).unwrap();
        assert_eq!(result, Some((101, 2000)));
    }

    #[test]
    fn insert_and_find_block_after_exclusive() {
        let (storage, _dir) = test_storage();
        storage
            .insert_blocks(1, &[100, 101, 102], &[1000, 2000, 3000])
            .unwrap();

        let result = storage.find_block(1, 2000, "after", false).unwrap();
        assert_eq!(result, Some((102, 3000)));
    }

    #[test]
    fn find_block_returns_none_when_no_match() {
        let (storage, _dir) = test_storage();

        let result = storage.find_block(1, 5000, "before", true).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn insert_blocks_is_idempotent() {
        let (storage, _dir) = test_storage();
        storage
            .insert_blocks(1, &[100, 101], &[1000, 2000])
            .unwrap();
        storage
            .insert_blocks(1, &[100, 101], &[1000, 2000])
            .unwrap();

        // count blocks for chain 1
        let count = storage.blocks.prefix(1u32.to_be_bytes()).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn cursor_round_trip() {
        let (storage, _dir) = test_storage();
        storage.upsert_cursor("ethereum-mainnet", 42).unwrap();

        let value = storage.get_cursor("ethereum-mainnet").unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn cursor_defaults_to_zero() {
        let (storage, _dir) = test_storage();

        let value = storage.get_cursor("nonexistent-slug").unwrap();
        assert_eq!(value, 0);
    }

    #[test]
    fn cursor_upsert_updates_existing() {
        let (storage, _dir) = test_storage();
        storage.upsert_cursor("ethereum-mainnet", 100).unwrap();
        storage.upsert_cursor("ethereum-mainnet", 200).unwrap();

        let value = storage.get_cursor("ethereum-mainnet").unwrap();
        assert_eq!(value, 200);
    }

    #[test]
    fn get_all_cursors_returns_all() {
        let (storage, _dir) = test_storage();
        storage.upsert_cursor("ethereum-mainnet", 100).unwrap();
        storage.upsert_cursor("base-mainnet", 200).unwrap();

        let mut cursors = storage.get_all_cursors().unwrap();
        cursors.sort_by(|a, b| a.0.cmp(&b.0));

        assert_eq!(cursors.len(), 2);
        assert_eq!(cursors[0].0, "base-mainnet");
        assert_eq!(cursors[0].1, 200);
        assert_eq!(cursors[1].0, "ethereum-mainnet");
        assert_eq!(cursors[1].1, 100);
    }

    #[test]
    fn chains_are_isolated() {
        let (storage, _dir) = test_storage();
        storage.insert_blocks(1, &[100], &[1000]).unwrap();
        storage.insert_blocks(2, &[200], &[2000]).unwrap();

        assert_eq!(
            storage.find_block(1, 5000, "before", true).unwrap(),
            Some((100, 1000))
        );
        assert_eq!(
            storage.find_block(2, 5000, "before", true).unwrap(),
            Some((200, 2000))
        );
        assert_eq!(storage.find_block(3, 5000, "before", true).unwrap(), None);
    }

    #[test]
    fn persist_does_not_error() {
        let (storage, _dir) = test_storage();
        storage.insert_blocks(1, &[1], &[100]).unwrap();
        storage.persist().unwrap();
    }
}
