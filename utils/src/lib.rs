use std::collections::HashSet;

use ethers::middleware::Middleware;
use ethers::signers::Signer;
use ethers::types::Address;
use once_cell::sync::Lazy;

pub mod logging;
pub mod utils;

pub static TOKEN_BLACKLIST: Lazy<HashSet<Address>> = Lazy::new(|| {
    let mut set = HashSet::new();
    set
});
