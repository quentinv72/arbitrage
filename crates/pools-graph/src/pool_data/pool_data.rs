use std::sync::Arc;

use enum_dispatch::enum_dispatch;
use ethers::providers::Middleware;
use ethers::types::{Address, Bytes, U256, U64};

use crate::pool_data::uniswap_v2::UniswapV2;
use crate::pool_data::uniswap_v3::UniswapV3;

#[enum_dispatch(PoolDataTrait)]
pub enum PoolData {
    UniswapV2,
    UniswapV3,
}

#[enum_dispatch]
pub trait PoolDataTrait {
    fn get_tokens(&self) -> (Address, Address);
    fn get_pool_address(&self) -> Address;
    fn get_last_block_update(&self) -> U64;
    fn get_amount_out(&self, amount_in: U256, zero_for_one: bool) -> U256;
    fn get_factory(&self) -> Address;
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
