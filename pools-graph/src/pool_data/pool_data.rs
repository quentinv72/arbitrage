use std::any::Any;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use ethers::providers::Middleware;
use ethers::types::{Address, Bytes, U256, U64};

pub trait PoolData: Send + Sync {
    fn get_tokens(&self) -> (Address, Address);
    fn get_pool_address(&self) -> Address;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn get_last_block_update(&self) -> U64;
    fn get_amount_out(&self, amount_in: U256, zero_for_one: bool) -> U256;
}

pub trait SwapBuilder {
    fn build_swap_calldata<M: Middleware>(
        &self,
        amount_in: U256,
        amount_out: U256,
        zero_for_one: bool,
        data: Bytes,
        client: Arc<M>,
        bundle_executor_address: Address,
    ) -> Bytes;
}

impl SwapBuilder for dyn PoolData {
    fn build_swap_calldata<M: Middleware>(
        &self,
        amount_in: U256,
        amount_out: U256,
        zero_for_one: bool,
        data: Bytes,
        client: Arc<M>,
        bundle_executor_address: Address,
    ) -> Bytes {
        todo!()
    }
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
