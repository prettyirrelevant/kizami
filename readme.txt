kizami
======

block-by-timestamp lookup API for EVM chains. ingests block headers from SQD Portal
into Postgres, then serves fast lookups from that index. supports 32 chains.

stack: rust, axum, postgres, moka (in-memory cache), tokio


data pipeline
-------------

                         NDJSON               UNNEST
    SQD Portal ---------> Ingestion Loop ---------> Postgres
    (32 chains)           (50k blocks/batch)         |
                          (cycles every 60s)         |
                                                     v
                                              +-------------+
    Client <--- JSON --- Axum API <---------- | blocks      |
                          |                   | cursors     |
                          v                   +-------------+
                     moka cache
                     (blocks: 30d TTL)
                     (cursors: 60s TTL)

ingestion loop and axum API run as a single binary.


ingestion cycle
---------------

for each of the 32 chains, every 60 seconds:

    read cursor from Postgres (last ingested block, default 0)
         |
         v
    fetch finalized head from SQD (cached 60s in moka)
         |
         v
    gap = head - cursor
         |
         +-- gap <= 0 ---> skip, chain is caught up
         |
         +-- gap > 0 ----> compute batch: [cursor+1, min(cursor+50k, head)]
                                |
                                v
                           POST /finalized-stream to SQD
                           parse NDJSON response (number, timestamp pairs)
                                |
                                v
                           INSERT via UNNEST (ON CONFLICT DO NOTHING)
                           upsert cursor to new position
                                |
                                v
                           update shared cursor cache (API reads this for indexedUpTo)

backfill happens naturally: new chains start at cursor 0, the loop sees the full
gap and chews through it in 50k-block batches.


block lookup
------------

    GET /v1/chains/:chainId/block/before/:timestamp

    check moka cache (key: "block:{chainId}:{ts}:{direction}")
         |
         +-- hit ----> return cached BlockResponse
         |
         +-- miss ---> query Postgres
                        |
                        v
                   SELECT number, timestamp
                   FROM blocks
                   WHERE chain_id = $1 AND timestamp < $2
                   ORDER BY timestamp DESC
                   LIMIT 1
                        |
                        v
                   fetch indexedUpTo from cursor cache
                        |
                        v
                   cache result (30-day TTL), return BlockResponse
                   { number, timestamp, indexedUpTo }


schema
------

    blocks
    +----------+---------+-----------+
    | chain_id | number  | timestamp |
    | INTEGER  | BIGINT  | BIGINT    |
    +----------+---------+-----------+
    PK: (chain_id, number)
    covering index: (chain_id, timestamp, number)  -- index-only scans

    cursors
    +----------+------------+------------+
    | sqd_slug | last_block | updated_at |
    | TEXT PK  | BIGINT     | TIMESTAMPTZ|
    +----------+------------+------------+


endpoints
---------

GET /v1/chains                                      list all supported chains
GET /v1/chains/:chainId                             get chain by ID
GET /v1/chains/:chainId/block/before/:timestamp     block before timestamp
GET /v1/chains/:chainId/block/after/:timestamp      block after timestamp
GET /health                                         health check
GET /docs                                           swagger UI


environment variables
---------------------

DATABASE_URL            postgres connection string (required)
PORT                    http port (default: 8080)
RUST_LOG                log level (default: info)
INGEST_INTERVAL_SECS    seconds between ingestion cycles (default: 60)


running locally
---------------

cargo build --release
DATABASE_URL=postgres://... ./target/release/kizami-api


project structure
-----------------

crates/
  shared/       chain configs, db queries, SQD client, error types, models
  api/          axum server, routes, state
  ingestion/    background ingestion loop
migrations/     postgres schema
