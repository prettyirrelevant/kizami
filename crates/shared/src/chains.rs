//! Static chain configuration for all supported EVM networks.
//!
//! All 30 chains are defined as compile-time constants with zero-allocation lookups
//! via `LazyLock<HashMap>`. Genesis timestamps are sourced from on-chain RPC
//! (`eth_getBlockByNumber`); where block 0 has timestamp 0, block 1 is used instead.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Configuration for a single EVM chain.
///
/// All fields are `&'static str` or Copy types, so lookups never allocate.
#[derive(Debug, Clone, Copy)]
pub struct ChainConfig {
    /// Human-readable chain name (e.g. "Ethereum", "Base").
    pub name: &'static str,
    /// EIP-155 chain ID.
    pub chain_id: i32,
    /// SQD Portal dataset slug used for API calls.
    /// See: <https://docs.sqd.dev/subsquid-network/reference/evm-networks/>
    pub sqd_slug: &'static str,
    /// Unix timestamp of the chain's genesis block (or block 1 if block 0 is 0).
    pub genesis_timestamp: i64,
}

/// All supported chains, ordered roughly by volume (heavy chains first).
pub static CHAINS: &[ChainConfig] = &[
    // high-volume chains
    ChainConfig {
        name: "Polygon",
        chain_id: 137,
        sqd_slug: "polygon-mainnet",
        genesis_timestamp: 1590824836,
    },
    ChainConfig {
        name: "BNB Smart Chain",
        chain_id: 56,
        sqd_slug: "binance-mainnet",
        genesis_timestamp: 1587390414,
    },
    ChainConfig {
        name: "Arbitrum One",
        chain_id: 42161,
        sqd_slug: "arbitrum-one",
        genesis_timestamp: 1622243344,
    },
    ChainConfig {
        name: "opBNB",
        chain_id: 204,
        sqd_slug: "opbnb-mainnet",
        genesis_timestamp: 1691753723,
    },
    // ethereum + medium chains
    ChainConfig {
        name: "Ethereum",
        chain_id: 1,
        sqd_slug: "ethereum-mainnet",
        genesis_timestamp: 1438269988,
    },
    ChainConfig {
        name: "Base",
        chain_id: 8453,
        sqd_slug: "base-mainnet",
        genesis_timestamp: 1686789347,
    },
    ChainConfig {
        name: "Optimism",
        chain_id: 10,
        sqd_slug: "optimism-mainnet",
        genesis_timestamp: 1636665399,
    },
    ChainConfig {
        name: "Avalanche",
        chain_id: 43114,
        sqd_slug: "avalanche-mainnet",
        genesis_timestamp: 1600858926,
    },
    ChainConfig {
        name: "Mantle",
        chain_id: 5000,
        sqd_slug: "mantle-mainnet",
        genesis_timestamp: 1688314886,
    },
    ChainConfig {
        name: "Gnosis",
        chain_id: 100,
        sqd_slug: "gnosis-mainnet",
        genesis_timestamp: 1539024185,
    },
    ChainConfig {
        name: "Linea",
        chain_id: 59144,
        sqd_slug: "linea-mainnet",
        genesis_timestamp: 1670496243,
    },
    ChainConfig {
        name: "Scroll",
        chain_id: 534352,
        sqd_slug: "scroll-mainnet",
        genesis_timestamp: 1696917600,
    },
    ChainConfig {
        name: "zkSync Era",
        chain_id: 324,
        sqd_slug: "zksync-mainnet",
        genesis_timestamp: 1676384542,
    },
    ChainConfig {
        name: "Sonic",
        chain_id: 146,
        sqd_slug: "sonic-mainnet",
        genesis_timestamp: 1733011200,
    },
    // lower-volume chains
    ChainConfig {
        name: "Manta Pacific",
        chain_id: 169,
        sqd_slug: "manta-pacific",
        genesis_timestamp: 1694223959,
    },
    ChainConfig {
        name: "Metis",
        chain_id: 1088,
        sqd_slug: "metis-mainnet",
        genesis_timestamp: 1637270379,
    },
    ChainConfig {
        name: "Blast",
        chain_id: 81457,
        sqd_slug: "blast-l2-mainnet",
        genesis_timestamp: 1708809815,
    },
    ChainConfig {
        name: "BOB",
        chain_id: 60808,
        sqd_slug: "bob-mainnet",
        genesis_timestamp: 1712861987,
    },
    ChainConfig {
        name: "Berachain",
        chain_id: 80094,
        sqd_slug: "berachain-mainnet",
        genesis_timestamp: 1737381600,
    },
    ChainConfig {
        name: "Unichain",
        chain_id: 130,
        sqd_slug: "unichain-mainnet",
        genesis_timestamp: 1730748359,
    },
    ChainConfig {
        name: "Flare",
        chain_id: 14,
        sqd_slug: "flare-mainnet",
        genesis_timestamp: 1657740761,
    },
    ChainConfig {
        name: "Etherlink",
        chain_id: 42793,
        sqd_slug: "etherlink-mainnet",
        genesis_timestamp: 1714656294,
    },
    ChainConfig {
        name: "Core",
        chain_id: 1116,
        sqd_slug: "core-mainnet",
        genesis_timestamp: 1637052000,
    },
    ChainConfig {
        name: "Taiko",
        chain_id: 167000,
        sqd_slug: "taiko-mainnet",
        genesis_timestamp: 1716620627,
    },
    ChainConfig {
        name: "Ink",
        chain_id: 57073,
        sqd_slug: "ink-mainnet",
        genesis_timestamp: 1733498411,
    },
    ChainConfig {
        name: "Plume",
        chain_id: 98866,
        sqd_slug: "plume-mainnet",
        genesis_timestamp: 1740047951,
    },
    ChainConfig {
        name: "Merlin",
        chain_id: 4200,
        sqd_slug: "merlin-mainnet",
        genesis_timestamp: 1706877604,
    },
    ChainConfig {
        name: "Celo",
        chain_id: 42220,
        sqd_slug: "celo-mainnet",
        genesis_timestamp: 1587571200,
    },
    ChainConfig {
        name: "Zora",
        chain_id: 7777777,
        sqd_slug: "zora-mainnet",
        genesis_timestamp: 1686693839,
    },
    ChainConfig {
        name: "Monad",
        chain_id: 143,
        sqd_slug: "monad-mainnet",
        genesis_timestamp: 1747232689,
    },
];

/// Lookup table from chain_id -> ChainConfig, built once on first access.
static CHAIN_BY_ID: LazyLock<HashMap<i32, &'static ChainConfig>> =
    LazyLock::new(|| CHAINS.iter().map(|c| (c.chain_id, c)).collect());

/// Lookup table from sqd_slug -> ChainConfig, built once on first access.
static CHAIN_BY_SLUG: LazyLock<HashMap<&'static str, &'static ChainConfig>> =
    LazyLock::new(|| CHAINS.iter().map(|c| (c.sqd_slug, c)).collect());

/// Returns the chain config for a given EIP-155 chain ID, or `None` if unsupported.
pub fn chain_by_id(chain_id: i32) -> Option<&'static ChainConfig> {
    CHAIN_BY_ID.get(&chain_id).copied()
}

/// Returns the chain config for a given SQD Portal dataset slug, or `None` if unsupported.
pub fn chain_by_slug(slug: &str) -> Option<&'static ChainConfig> {
    CHAIN_BY_SLUG.get(slug).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_id() {
        let eth = chain_by_id(1).unwrap();
        assert_eq!(eth.name, "Ethereum");
        assert_eq!(eth.sqd_slug, "ethereum-mainnet");
    }

    #[test]
    fn lookup_by_slug() {
        let base = chain_by_slug("base-mainnet").unwrap();
        assert_eq!(base.chain_id, 8453);
        assert_eq!(base.name, "Base");
    }

    #[test]
    fn missing_chain_returns_none() {
        assert!(chain_by_id(999999).is_none());
        assert!(chain_by_slug("nonexistent").is_none());
    }

    #[test]
    fn all_chains_have_unique_ids() {
        let mut ids: Vec<i32> = CHAINS.iter().map(|c| c.chain_id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CHAINS.len());
    }

    #[test]
    fn all_chains_have_unique_slugs() {
        let mut slugs: Vec<&str> = CHAINS.iter().map(|c| c.sqd_slug).collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), CHAINS.len());
    }
}
