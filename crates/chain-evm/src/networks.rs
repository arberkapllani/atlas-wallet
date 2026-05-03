//! Static descriptors for every supported EVM network.
//!
//! Default RPC URLs are public endpoints — users can override per-network
//! in settings to point at their own node.

/// A description of an EVM-compatible network.
#[derive(Debug, Clone, Copy)]
pub struct Network {
    /// Stable identifier, lowercase (`eth`, `polygon`, …).
    pub id: &'static str,
    /// Display name (`Ethereum`, `Polygon`, …).
    pub display_name: &'static str,
    /// Native-asset ticker (`ETH`, `MATIC`, `BNB`, …).
    pub symbol: &'static str,
    /// EIP-155 chain id.
    pub chain_id: u64,
    /// Default JSON-RPC endpoint.
    pub rpc_url: &'static str,
    /// Block explorer base URL (without trailing slash).
    pub explorer: &'static str,
    /// Whether this chain is enabled by default in the UI.
    pub enabled_by_default: bool,
}

/// All supported EVM networks. Order is the UI display order.
pub const NETWORKS: &[Network] = &[
    Network {
        id: "eth",
        display_name: "Ethereum",
        symbol: "ETH",
        chain_id: 1,
        rpc_url: "https://eth.llamarpc.com",
        explorer: "https://etherscan.io",
        enabled_by_default: true,
    },
    Network {
        id: "polygon",
        display_name: "Polygon",
        symbol: "MATIC",
        chain_id: 137,
        rpc_url: "https://polygon-rpc.com",
        explorer: "https://polygonscan.com",
        enabled_by_default: true,
    },
    Network {
        id: "arbitrum",
        display_name: "Arbitrum One",
        symbol: "ETH",
        chain_id: 42161,
        rpc_url: "https://arb1.arbitrum.io/rpc",
        explorer: "https://arbiscan.io",
        enabled_by_default: true,
    },
    Network {
        id: "optimism",
        display_name: "Optimism",
        symbol: "ETH",
        chain_id: 10,
        rpc_url: "https://mainnet.optimism.io",
        explorer: "https://optimistic.etherscan.io",
        enabled_by_default: true,
    },
    Network {
        id: "base",
        display_name: "Base",
        symbol: "ETH",
        chain_id: 8453,
        rpc_url: "https://mainnet.base.org",
        explorer: "https://basescan.org",
        enabled_by_default: true,
    },
    Network {
        id: "bsc",
        display_name: "BNB Smart Chain",
        symbol: "BNB",
        chain_id: 56,
        rpc_url: "https://bsc-dataseed.binance.org",
        explorer: "https://bscscan.com",
        enabled_by_default: false,
    },
    Network {
        id: "avalanche",
        display_name: "Avalanche C-Chain",
        symbol: "AVAX",
        chain_id: 43114,
        rpc_url: "https://api.avax.network/ext/bc/C/rpc",
        explorer: "https://snowtrace.io",
        enabled_by_default: false,
    },
];

/// Lookup a network by its [`Network::id`].
pub fn by_id(id: &str) -> Option<&'static Network> {
    NETWORKS.iter().find(|n| n.id == id)
}
