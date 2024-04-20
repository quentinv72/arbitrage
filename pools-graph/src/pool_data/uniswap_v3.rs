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
}

impl PoolData for UniswapV3 {
    fn get_tokens(&self) -> (Address, Address) {
        (self.token_0, self.token_1)
    }

    fn get_pool_address(&self) -> Address {
        self.pool_address
    }
}