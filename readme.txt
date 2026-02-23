kizami
======

block-by-timestamp lookup API for EVM chains. ingests block headers from SQD Portal
into Postgres, then serves fast lookups from that index. supports 32 chains.

stack: rust, axum, postgres, moka (in-memory cache), tokio


diagrams
--------

architecture overview:
https://excalidraw.com/#json=X8O5sduqz8SWifNpkSacb,MeFUc3U7lDK38cOQp5JLnA

block lookup sequence:
https://excalidraw.com/#json=bZFTZ3CTNslEStyzUAA8_,dIUEToXdSaWjz5Z_LsxSkg


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
docs/           architecture doc
