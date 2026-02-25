kizami
======

block-by-timestamp lookup API for EVM chains. ingests block headers from SQD Portal
into an embedded fjall database, then serves fast lookups from that index. supports
30 chains. single binary, no external dependencies.

stack: rust, axum, fjall (embedded LSM-tree storage), tokio


data pipeline
-------------

                         NDJSON
    SQD Portal ---------> Ingestion Loop ---------> fjall
    (30 chains)           (50k blocks/batch)         |
                          (cycles every 60s)         |
                                                     v
                                              +-------------+
    Client <--- JSON --- Axum API <---------- | blocks KS   |
                          |                   | cursors KS  |
                          v                   +-------------+
                     in-memory progress map
                     (cursor + head per chain)

ingestion loop and axum API run as a single binary.


ingestion cycle
---------------

for each of the 30 chains, every 60 seconds:

    read cursor from progress map (last ingested block, default 0)
         |
         v
    fetch finalized head from SQD (stale value used as fallback)
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
                           write block keys to fjall (idempotent)
                           upsert cursor to new position
                                |
                                v
                           update shared progress map (API reads this for indexedUpTo)

backfill happens naturally: new chains start at cursor 0, the loop sees the full
gap and chews through it in 50k-block batches.


block lookup
------------

    GET /v1/chains/:chainId/block/before/:timestamp

    range scan on fjall blocks keyspace
         |
         v
    key = chain_id(4B BE) | timestamp(8B BE) | number(8B BE)
    big-endian ensures lexicographic order matches numeric order
         |
         v
    before inclusive: range(C|0|0 ..= C|T|MAX).next_back()
    before exclusive: range(C|0|0 .. C|T|0).next_back()
    after inclusive:  range(C|T|0 .. C+1|0|0).next()
    after exclusive:  range(C|T+1|0 .. C+1|0|0).next()
         |
         v
    read indexedUpTo from progress map
         |
         v
    return BlockResponse { number, timestamp, indexedUpTo }


storage layout
--------------

    blocks keyspace
    key: chain_id (4B u32 BE) | timestamp (8B u64 BE) | number (8B u64 BE) = 20 bytes
    value: empty

    cursors keyspace
    key: sqd_slug (UTF-8 string)
    value: last_block (8B i64 BE) | updated_at_secs (8B i64 BE) = 16 bytes


endpoints
---------

GET /v1/chains                                      list all supported chains
GET /v1/chains/:chainId                             get chain by ID
GET /v1/chains/:chainId/block/before/:timestamp     block before timestamp
GET /v1/chains/:chainId/block/after/:timestamp      block after timestamp
GET /v1/indexing-status                             indexing progress for all chains
GET /health                                         health check
GET /docs                                           swagger UI


environment variables
---------------------

DATA_DIR                path to fjall data directory (default: ./data)
PORT                    http port (default: 8080)
RUST_LOG                log level (default: info)
INGEST_INTERVAL_SECS    seconds between ingestion cycles (default: 60)


running locally
---------------

cargo build --release
./target/release/kizami-api

data is stored in ./data by default. override with DATA_DIR.


project structure
-----------------

crates/
  shared/       chain configs, storage layer, SQD client, error types, models
  api/          axum server, routes, state
  ingestion/    background ingestion loop
