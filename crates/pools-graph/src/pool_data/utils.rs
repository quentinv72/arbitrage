use ethers::middleware::Middleware;
use revm::db::{CacheDB, EthersDB};

#[allow(type_alias_bounds)]
pub type EthersCacheDB<M: Middleware> = CacheDB<EthersDB<M>>;
