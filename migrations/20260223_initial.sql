CREATE TABLE IF NOT EXISTS blocks (
    chain_id  INTEGER NOT NULL,
    number    BIGINT  NOT NULL,
    timestamp BIGINT  NOT NULL,
    PRIMARY KEY (chain_id, number)
);

CREATE INDEX idx_blocks_chain_ts ON blocks (chain_id, timestamp, number);

CREATE TABLE IF NOT EXISTS cursors (
    sqd_slug   TEXT PRIMARY KEY,
    last_block BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
