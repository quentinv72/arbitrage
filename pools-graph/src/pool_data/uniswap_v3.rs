use std::any::Any;
use std::sync::Arc;

use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;
use ethers::contract::ContractError;
use ethers::middleware::Middleware;
use ethers::types::{Address, U256};

use crate::pool_data::pool_data::PoolData;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct UniswapV3 {
    pool_address: Address,
    quoter_address: Address,
    sqrt_price_x_96: U256,
    token_0: Address,
    token_1: Address,
    fee_tier: u32,
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
        })
    }
}

impl PoolData for UniswapV3 {
    fn get_tokens(&self) -> (Address, Address) {
        (self.token_0, self.token_1)
    }

    fn get_pool_address(&self) -> Address {
        self.pool_address
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}