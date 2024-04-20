use std::sync::Arc;

use contracts::i_uniswap_v_2_pair::IUniswapV2Pair;
use ethers::middleware::Middleware;
use ethers::prelude::ContractError;
use ethers::types::{Address, U64};

use crate::pool_data::pool_data::PoolData;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct UniswapV2 {
    pair_address: Address,
    token_0: Address,
    reserve_0: u128,
    token_1: Address,
    reserve_1: u128,
    block_last_updated: U64,
}

impl UniswapV2 {
    pub fn new(
        pair_address: Address,
        token_0: Address,
        reserve_0: u128,
        token_1: Address,
        reserve_1: u128,
        block_last_updated: U64,
    ) -> Self {
        Self {
            pair_address,
            token_0,
            reserve_0,
            token_1,
            reserve_1,
            block_last_updated,
        }
    }

    pub async fn new_from_client<M: Middleware>(pair_address: Address, client: Arc<M>) -> Result<UniswapV2, ContractError<M>> {
        let pool_contract = IUniswapV2Pair::new(pair_address, client);
        let token_0 = pool_contract.token_0().call().await?;
        let token_1 = pool_contract.token_1().call().await?;
        let (reserve_0, reserve_1, _) = pool_contract.get_reserves().call().await?;
        Ok(
            Self {
                pair_address,
                token_0,
                token_1,
                reserve_0,
                reserve_1,
                block_last_updated: U64::zero(),
            }
        )
    }
}

impl PoolData for UniswapV2 {
    fn get_tokens(&self) -> (Address, Address) {
        (self.token_0, self.token_1)
    }

    fn get_pool_address(&self) -> Address {
        self.pair_address
    }
}