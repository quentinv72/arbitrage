use std::any::Any;
use std::ops::{Div, Mul};
use std::sync::Arc;

use contracts::i_uniswap_v_2_pair::IUniswapV2Pair;
use ethers::middleware::Middleware;
use ethers::prelude::{Bytes, ContractError, U256};
use ethers::types::{Address, U64};
use log::warn;

use crate::pool_data::pool_data::{PoolData, PoolDataTrait};

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

    pub fn get_reserves(&self) -> (u128, u128) {
        (self.reserve_0, self.reserve_1)
    }

    pub async fn new_from_client<M: Middleware>(
        pair_address: Address,
        client: Arc<M>,
    ) -> Result<UniswapV2, ContractError<M>> {
        let pool_contract = IUniswapV2Pair::new(pair_address, client);
        let token_0 = pool_contract.token_0().call().await?;
        let token_1 = pool_contract.token_1().call().await?;
        let (reserve_0, reserve_1, _) = pool_contract.get_reserves().call().await?;
        Ok(Self {
            pair_address,
            token_0,
            token_1,
            reserve_0,
            reserve_1,
            block_last_updated: U64::zero(),
        })
    }

    pub async fn maybe_refresh_reserves<M: Middleware>(
        &mut self,
        current_block: U64,
        client: Arc<M>,
    ) -> Result<(), ContractError<M>> {
        if self.block_last_updated == current_block {
            Ok(())
        } else {
            let pair_contract = IUniswapV2Pair::new(self.pair_address, client);
            let (reserve_0, reserve_1, _) = pair_contract.get_reserves().call().await?;
            self.reserve_0 = reserve_0;
            self.reserve_1 = reserve_1;
            self.block_last_updated = current_block;
            Ok(())
        }
    }

    fn get_amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> U256 {
        let amount_in_with_fee = amount_in.mul(U256::from(997));
        let numerator = amount_in_with_fee.mul(reserve_out);
        let denominator = (reserve_in.mul(U256::from(1_000))) + (amount_in_with_fee);
        if denominator.is_zero() {
            warn!("amount in: {amount_in}, reserve in: {reserve_in}, reserve out: {reserve_out}");
            return U256::from(0);
        }
        numerator.div(denominator)
    }
}

impl PoolDataTrait for UniswapV2 {
    fn get_tokens(&self) -> (Address, Address) {
        (self.token_0, self.token_1)
    }

    fn get_pool_address(&self) -> Address {
        self.pair_address
    }

    fn get_last_block_update(&self) -> U64 {
        self.block_last_updated
    }

    fn get_amount_out(&self, amount_in: U256, zero_for_one: bool) -> U256 {
        let reserve_in = if zero_for_one {
            self.reserve_0
        } else {
            self.reserve_1
        };
        let reserve_out = if zero_for_one {
            self.reserve_1
        } else {
            self.reserve_0
        };
        Self::get_amount_out(amount_in, U256::from(reserve_in), U256::from(reserve_out))
    }

    fn build_swap_calldata<M: Middleware>(
        &self,
        _amount_in: U256,
        amount_out: U256,
        zero_for_one: bool,
        data: Bytes,
        client: Arc<M>,
        bundle_executor_address: Address,
    ) -> Bytes {
        let uniswap_v2_contract = IUniswapV2Pair::new(self.pair_address, client);
        if zero_for_one {
            uniswap_v2_contract
                .swap(U256::zero(), amount_out, bundle_executor_address, data)
                .calldata()
                .unwrap()
        } else {
            uniswap_v2_contract
                .swap(amount_out, U256::zero(), bundle_executor_address, data)
                .calldata()
                .unwrap()
        }
    }
}
