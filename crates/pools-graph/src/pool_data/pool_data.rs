use std::sync::Arc;

use enum_dispatch::enum_dispatch;
use ethers::abi;
use ethers::abi::RawLog;
use ethers::contract::ContractError;
use ethers::providers::Middleware;
use ethers::types::{Address, Bytes, Log, U256, U64};

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
    #[allow(async_fn_in_trait)]
    async fn update_pool<M: Middleware>(&mut self, client: Arc<M>) -> Result<(), ContractError<M>>;
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

impl PoolData {
    #[inline]
    pub(crate) fn new_pool(log: Log) -> Result<Option<Self>, abi::Error> {
        if let Some(event_sig) = log.topics.first() {
            if *event_sig == UniswapV2::get_new_pool_sig() {
                return Ok(Some(PoolData::UniswapV2(UniswapV2::new_from_log(
                    &RawLog::from(log),
                )?)));
            } else if *event_sig == UniswapV3::get_new_pool_sig() {
                return Ok(Some(PoolData::UniswapV3(UniswapV3::new_from_log(
                    &RawLog::from(log),
                )?)));
            }
        }
        Ok(None)
    }
}
