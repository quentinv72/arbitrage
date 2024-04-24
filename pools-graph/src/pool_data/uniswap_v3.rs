use std::any::Any;
use std::sync::Arc;

use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;
use ethers::contract::ContractError;
use ethers::middleware::Middleware;
use ethers::prelude::U64;
use ethers::types::{Address, Bytes, U256};

use crate::pool_data::pool_data::PoolDataTrait;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct UniswapV3 {
    pool_address: Address,
    quoter_address: Address,
    sqrt_price_x_96: U256,
    token_0: Address,
    token_1: Address,
    fee_tier: u32,
    block_last_updates: U64,
}

impl UniswapV3 {
    pub fn new(
        pool_address: Address,
        quoter_address: Address,
        sqrt_price_x_96: U256,
        token_0: Address,
        token_1: Address,
        fee_tier: u32,
    ) -> Self {
        Self {
            pool_address,
            quoter_address,
            sqrt_price_x_96,
            token_0,
            token_1,
            fee_tier,
            block_last_updates: U64::zero(),
        }
    }

    pub async fn new_from_client<M: Middleware>(
        pool_address: Address,
        quoter_address: Address,
        token_0: Address,
        token_1: Address,
        fee_tier: u32,
        client: Arc<M>,
    ) -> Result<Self, ContractError<M>> {
        let pool_contract = IUniswapV3Pool::new(pool_address, client);
        let (sqrt_price_x_96, _, _, _, _, _, _) = pool_contract.slot_0().call().await?;
        Ok(Self {
            pool_address,
            quoter_address,
            sqrt_price_x_96,
            token_0,
            token_1,
            fee_tier,
            block_last_updates: U64::zero(),
        })
    }
}

impl PoolDataTrait for UniswapV3 {
    fn get_tokens(&self) -> (Address, Address) {
        (self.token_0, self.token_1)
    }

    fn get_pool_address(&self) -> Address {
        self.pool_address
    }

    fn get_last_block_update(&self) -> U64 {
        self.block_last_updates
    }

    fn get_amount_out(&self, amount_in: U256, zero_for_one: bool) -> U256 {
        todo!("V3 amount out not implemented")
    }

    fn build_swap_calldata<M: Middleware>(
        &self,
        amount_in: U256,
        amount_out: U256,
        zero_for_one: bool,
        data: Bytes,
        client: Arc<M>,
        bundle_executor_address: Address,
    ) -> Bytes {
        todo!("V3 swap calldata not implemented")
    }
}
