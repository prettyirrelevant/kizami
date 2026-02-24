//! Database operations for blocks and cursors.
//!
//! Uses sqlx with Postgres. Block inserts use `UNNEST` for efficient bulk loading,
//! and lookups use the covering index `(chain_id, timestamp, number)` for index-only scans.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Creates a connection pool with up to 20 connections.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
}

/// Runs pending migrations from the `migrations/` directory.
///
/// Uses sqlx's built-in migration tracking (`_sqlx_migrations` table) so each
/// migration only runs once.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}

/// Finds the closest block to a given timestamp in the specified direction.
///
/// Returns `(number, timestamp)` of the matching block, or `None` if no block matches.
/// The four match arms handle the combinations of `before/after` and `inclusive/exclusive`,
/// using the covering index for efficient range scans.
pub async fn find_block(
    pool: &PgPool,
    chain_id: i32,
    timestamp: i64,
    direction: &str,
    inclusive: bool,
) -> Result<Option<(i64, i64)>, sqlx::Error> {
    let row: Option<(i64, i64)> = match (direction, inclusive) {
        ("before", true) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp <= $2 \
                 ORDER BY timestamp DESC, number DESC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        ("before", false) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp < $2 \
                 ORDER BY timestamp DESC, number DESC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        ("after", true) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp >= $2 \
                 ORDER BY timestamp ASC, number ASC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        ("after", false) => {
            sqlx::query_as(
                "SELECT number, timestamp FROM blocks \
                 WHERE chain_id = $1 AND timestamp > $2 \
                 ORDER BY timestamp ASC, number ASC LIMIT 1",
            )
            .bind(chain_id)
            .bind(timestamp)
            .fetch_optional(pool)
            .await?
        }
        _ => None,
    };
    Ok(row)
}

/// Bulk-inserts blocks using `UNNEST` for efficient batch loading.
///
/// Uses `ON CONFLICT DO NOTHING` for idempotency, so re-ingesting the same range is safe.
/// The `numbers` and `timestamps` slices must have the same length.
pub async fn insert_blocks(
    pool: &PgPool,
    chain_id: i32,
    numbers: &[i64],
    timestamps: &[i64],
) -> Result<u64, sqlx::Error> {
    let chain_ids: Vec<i32> = vec![chain_id; numbers.len()];
    let result = sqlx::query(
        "INSERT INTO blocks (chain_id, number, timestamp) \
         SELECT * FROM UNNEST($1::int[], $2::bigint[], $3::bigint[]) \
         ON CONFLICT (chain_id, number) DO NOTHING",
    )
    .bind(&chain_ids)
    .bind(numbers)
    .bind(timestamps)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Returns the last ingested block number for a chain, or 0 if no cursor exists.
pub async fn get_cursor(pool: &PgPool, sqd_slug: &str) -> Result<i64, sqlx::Error> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT last_block FROM cursors WHERE sqd_slug = $1")
        .bind(sqd_slug)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.0).unwrap_or(0))
}

/// Upserts the ingestion cursor for a chain.
///
/// Inserts if no cursor exists, otherwise updates `last_block` and `updated_at`.
pub async fn upsert_cursor(
    pool: &PgPool,
    sqd_slug: &str,
    last_block: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO cursors (sqd_slug, last_block, updated_at) \
         VALUES ($1, $2, now()) \
         ON CONFLICT (sqd_slug) DO UPDATE SET last_block = $2, updated_at = now()",
    )
    .bind(sqd_slug)
    .bind(last_block)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use sqlx::postgres::PgPoolOptions;
    use sqlx::{Executor, PgPool};
    use testcontainers_modules::postgres::Postgres;
    use testcontainers_modules::testcontainers::runners::AsyncRunner;
    use testcontainers_modules::testcontainers::ContainerAsync;
    use tokio::sync::OnceCell;

    use super::*;

    struct TestInfra {
        _container: ContainerAsync<Postgres>,
        base_url: String,
    }

    static INFRA: OnceCell<TestInfra> = OnceCell::const_new();

    async fn init_infra() -> &'static TestInfra {
        INFRA
            .get_or_init(|| async {
                let container = Postgres::default()
                    .with_db_name("postgres")
                    .with_user("postgres")
                    .with_password("postgres")
                    .start()
                    .await
                    .expect("failed to start postgres container");

                let host = container.get_host().await.expect("failed to get host");
                let port = container
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("failed to get port");
                let base_url = format!("postgres://postgres:postgres@{host}:{port}");

                let pool = PgPoolOptions::new()
                    .max_connections(2)
                    .connect(&format!("{base_url}/postgres"))
                    .await
                    .expect("failed to connect to postgres");

                pool.execute("DROP DATABASE IF EXISTS template_kizami")
                    .await
                    .expect("failed to drop template db");
                pool.execute("CREATE DATABASE template_kizami")
                    .await
                    .expect("failed to create template db");

                let tpl_pool = PgPoolOptions::new()
                    .max_connections(2)
                    .connect(&format!("{base_url}/template_kizami"))
                    .await
                    .expect("failed to connect to template db");
                run_migrations(&tpl_pool)
                    .await
                    .expect("failed to run migrations");
                tpl_pool.close().await;

                TestInfra {
                    _container: container,
                    base_url,
                }
            })
            .await
    }

    pub async fn test_pool() -> PgPool {
        let infra = init_infra().await;
        let db_name = format!("test_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));

        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&format!("{}/postgres", infra.base_url))
            .await
            .expect("failed to connect for db creation");

        pool.execute(format!("CREATE DATABASE {db_name} TEMPLATE template_kizami").as_str())
            .await
            .expect("failed to create test db");
        pool.close().await;

        PgPoolOptions::new()
            .max_connections(5)
            .connect(&format!("{}/{db_name}", infra.base_url))
            .await
            .expect("failed to connect to test db")
    }

    #[tokio::test]
    async fn insert_and_find_block_before_inclusive() {
        let pool = test_pool().await;
        insert_blocks(&pool, 1, &[100, 101, 102], &[1000, 2000, 3000])
            .await
            .unwrap();

        let result = find_block(&pool, 1, 2000, "before", true).await.unwrap();
        assert_eq!(result, Some((101, 2000)));
    }

    #[tokio::test]
    async fn insert_and_find_block_before_exclusive() {
        let pool = test_pool().await;
        insert_blocks(&pool, 1, &[100, 101, 102], &[1000, 2000, 3000])
            .await
            .unwrap();

        let result = find_block(&pool, 1, 2000, "before", false).await.unwrap();
        assert_eq!(result, Some((100, 1000)));
    }

    #[tokio::test]
    async fn insert_and_find_block_after_inclusive() {
        let pool = test_pool().await;
        insert_blocks(&pool, 1, &[100, 101, 102], &[1000, 2000, 3000])
            .await
            .unwrap();

        let result = find_block(&pool, 1, 2000, "after", true).await.unwrap();
        assert_eq!(result, Some((101, 2000)));
    }

    #[tokio::test]
    async fn insert_and_find_block_after_exclusive() {
        let pool = test_pool().await;
        insert_blocks(&pool, 1, &[100, 101, 102], &[1000, 2000, 3000])
            .await
            .unwrap();

        let result = find_block(&pool, 1, 2000, "after", false).await.unwrap();
        assert_eq!(result, Some((102, 3000)));
    }

    #[tokio::test]
    async fn find_block_returns_none_when_no_match() {
        let pool = test_pool().await;

        let result = find_block(&pool, 1, 5000, "before", true).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn insert_blocks_is_idempotent() {
        let pool = test_pool().await;
        insert_blocks(&pool, 1, &[100, 101], &[1000, 2000])
            .await
            .unwrap();
        insert_blocks(&pool, 1, &[100, 101], &[1000, 2000])
            .await
            .unwrap();

        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM blocks WHERE chain_id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, 2);
    }

    #[tokio::test]
    async fn cursor_round_trip() {
        let pool = test_pool().await;
        upsert_cursor(&pool, "ethereum-mainnet", 42).await.unwrap();

        let value = get_cursor(&pool, "ethereum-mainnet").await.unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn cursor_defaults_to_zero() {
        let pool = test_pool().await;

        let value = get_cursor(&pool, "nonexistent-slug").await.unwrap();
        assert_eq!(value, 0);
    }

    #[tokio::test]
    async fn cursor_upsert_updates_existing() {
        let pool = test_pool().await;
        upsert_cursor(&pool, "ethereum-mainnet", 100).await.unwrap();
        upsert_cursor(&pool, "ethereum-mainnet", 200).await.unwrap();

        let value = get_cursor(&pool, "ethereum-mainnet").await.unwrap();
        assert_eq!(value, 200);
    }
}
