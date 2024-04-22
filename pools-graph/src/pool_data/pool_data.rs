use std::any::Any;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use ethers::types::{Address, U64};

pub trait PoolData: Send + Sync {
    fn get_tokens(&self) -> (Address, Address);
    fn get_pool_address(&self) -> Address;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn get_last_block_update(&self) -> U64;
}

impl Hash for dyn PoolData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.get_pool_address().hash(state);
    }
}

impl PartialEq for dyn PoolData {
    fn eq(&self, other: &Self) -> bool {
        self.get_pool_address() == other.get_pool_address()
    }
}

impl Eq for dyn PoolData {}