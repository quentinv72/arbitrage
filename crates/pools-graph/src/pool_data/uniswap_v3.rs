use std::sync::Arc;

use ethers::contract::ContractError;
use ethers::middleware::Middleware;
use ethers::prelude::U64;
use ethers::types::{Address, Bytes, U256};

use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;

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

    fn get_amount_out(&self, _amount_in: U256, _zero_for_one: bool) -> U256 {
        todo!("V3 amount out not implemented")
    }

    fn build_swap_calldata<M: Middleware>(
        &self,
        _amount_in: U256,
        _amount_out: U256,
        _zero_for_one: bool,
        _data: Bytes,
        _client: Arc<M>,
        _bundle_executor_address: Address,
    ) -> Bytes {
        todo!("V3 swap calldata not implemented")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ethers::prelude::{Http, Provider};
    use ethers::types::{Address, U64};

    use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;

    use crate::pool_data::uniswap_v3::UniswapV3;

    fn get_client() -> Provider<Http> {
        Provider::<Http>::try_from(
            "https://eth-sepolia.g.alchemy.com/v2/fEmCuDGqB-tSA4R5HnnVCy1n9Jg4GqJg",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn new_from_client() {
        let pool_address: Address = "0xE7361b7375Cb8e794A97788Ef63E95Fc03221320"
            .parse()
            .unwrap();
        let client = Arc::new(get_client());
        let pool_contract = IUniswapV3Pool::new(pool_address, client.clone());
        let quoter = Address::random();
        let token_0 = Address::random();
        let token_1 = Address::random();
        let fee_tier = 5;
        let (sqrt_price_x_96, _, _, _, _, _, _) = pool_contract.slot_0().await.unwrap();
        let pool_data =
            UniswapV3::new_from_client(pool_address, quoter, token_0, token_1, fee_tier, client)
                .await
                .unwrap();
        assert_eq!(pool_data.pool_address, pool_address);
        assert_eq!(pool_data.token_0, token_0);
        assert_eq!(pool_data.token_1, token_1);
        assert_eq!(pool_data.fee_tier, fee_tier);
        assert_eq!(pool_data.sqrt_price_x_96, sqrt_price_x_96);
        assert_eq!(pool_data.block_last_updates, U64::zero());
    }
}
