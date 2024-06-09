use ethers::providers::Middleware;
use revm::db::{CacheDB, EthersDB};

pub mod arbitrage;
pub mod uniswap_v2;
pub mod uniswap_v3;

#[allow(type_alias_bounds)]
pub type EthersCacheDB<M: Middleware> = CacheDB<EthersDB<M>>;
