use std::ops::{Div, Mul};
use std::sync::Arc;

use ethers::abi;
use ethers::abi::{AbiEncode, RawLog};
use ethers::contract::EthEvent;
use ethers::middleware::Middleware;
use ethers::prelude::{Bytes, ContractError, H256, U256};
use ethers::types::{Address, U64};

use contracts::i_uniswap_v_2_factory::PairCreatedFilter;
use contracts::i_uniswap_v_2_pair::{IUniswapV2Pair, SwapCall};

use crate::pool_data::factory::{Factory, FactoryV2};
use crate::pool_data::pool_data::PoolDataTrait;

pub mod utils;

#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct UniswapV2 {
    pub pair_address: Address,
    pub token_0: Address,
    pub reserve_0: u128,
    pub token_1: Address,
    pub reserve_1: u128,
    pub block_last_updated: U64,
    pub factory: FactoryV2,
}

impl UniswapV2 {
    pub fn get_reserves(&self) -> (u128, u128) {
        (self.reserve_0, self.reserve_1)
    }

    pub async fn new_from_client<M: Middleware>(
        pair_address: Address,
        factory: FactoryV2,
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
            factory,
        })
    }

    pub(crate) fn get_new_pool_sig() -> H256 {
        PairCreatedFilter::signature()
    }

    pub(crate) fn new_from_log(log: &RawLog, factory_v2: FactoryV2) -> Result<Self, abi::Error> {
        let parsed_event = PairCreatedFilter::decode_log(log)?;
        Ok(Self {
            pair_address: parsed_event.pair,
            token_0: parsed_event.token_0,
            token_1: parsed_event.token_1,
            factory: factory_v2,
            ..Default::default()
        })
    }

    pub async fn maybe_refresh_reserves<M: Middleware>(
        &mut self,
        current_block: Option<U64>,
        client: Arc<M>,
    ) -> Result<(), ContractError<M>> {
        if current_block.is_some() && self.block_last_updated == current_block.unwrap() {
            Ok(())
        } else {
            let pair_contract = IUniswapV2Pair::new(self.pair_address, client);
            let (reserve_0, reserve_1, _) = pair_contract.get_reserves().call().await?;
            self.reserve_0 = reserve_0;
            self.reserve_1 = reserve_1;
            self.block_last_updated = current_block.unwrap_or_default();
            Ok(())
        }
    }

    // this is only used by the pools graph tests
    pub fn new(
        pair_address: Address,
        token_0: Address,
        reserve_0: u128,
        token_1: Address,
        reserve_1: u128,
        block_last_updated: U64,
        factory: Option<FactoryV2>,
    ) -> Self {
        Self {
            pair_address,
            token_0,
            reserve_0,
            token_1,
            reserve_1,
            block_last_updated,
            factory: factory.unwrap_or_default(),
        }
    }

    #[inline]
    fn get_amount_out(
        amount_in: U256,
        reserve_in: U256,
        reserve_out: U256,
        swap_fee: U256,
    ) -> U256 {
        let calc_fee = U256::from(1_000) - swap_fee;
        let amount_in_with_fee = amount_in.mul(calc_fee);
        let numerator = amount_in_with_fee.mul(reserve_out);
        let denominator = (reserve_in.mul(U256::from(1_000))) + (amount_in_with_fee);
        if denominator.is_zero() {
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

    fn get_factory(&self) -> Factory {
        self.factory.into()
    }

    #[inline]
    async fn get_amount_out<M: Middleware>(
        &self,
        amount_in: U256,
        token_in: Address,
        _token_out: Address,
        _client: Option<Arc<M>>,
    ) -> anyhow::Result<U256> {
        let zero_for_one = token_in == self.token_0;
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
        Ok(Self::get_amount_out(
            amount_in,
            U256::from(reserve_in),
            U256::from(reserve_out),
            self.factory.swap_fee,
        ))
    }

    #[inline]
    fn build_swap_calldata(
        &self,
        _amount_in: U256,
        amount_out: U256,
        token_in: Address,
        _token_out: Address,
        data: Bytes,
        bundle_executor_address: Address,
    ) -> Bytes {
        let zero_for_one = token_in == self.token_0;
        if zero_for_one {
            SwapCall {
                amount_0_out: U256::zero(),
                amount_1_out: amount_out,
                to: bundle_executor_address,
                data,
            }
            .encode()
            .into()
        } else {
            SwapCall {
                amount_0_out: amount_out,
                amount_1_out: U256::zero(),
                to: bundle_executor_address,
                data,
            }
            .encode()
            .into()
        }
    }

    async fn update_pool<M: Middleware>(&mut self, client: Arc<M>) -> Result<(), ContractError<M>> {
        self.maybe_refresh_reserves(None, client).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ethers::providers::{Http, Provider};
    use ethers::types::{Address, U256, U64};

    use contracts::i_uniswap_v_2_pair::IUniswapV2Pair;
    use utils::placeholder_middleware::PlaceholderMiddleware;

    use crate::pool_data::factory::FactoryV2;
    use crate::pool_data::pool_data::PoolDataTrait;
    use crate::pool_data::uniswap_v2::UniswapV2;

    fn get_client() -> Provider<Http> {
        Provider::<Http>::try_from(
            "https://eth-sepolia.g.alchemy.com/v2/fEmCuDGqB-tSA4R5HnnVCy1n9Jg4GqJg",
        )
        .unwrap()
    }

    fn create_pool_data() -> UniswapV2 {
        UniswapV2 {
            pair_address: Address::zero(),
            token_0: Address::random(),
            token_1: Address::random(),
            reserve_0: 1000,
            reserve_1: 100000000,
            block_last_updated: U64::one(),
            factory: FactoryV2 {
                address: Address::random(),
                swap_fee: U256::from(3),
            },
        }
    }

    #[test]
    fn reserves() {
        let pool_data = create_pool_data();
        assert_eq!(
            pool_data.get_reserves(),
            (pool_data.reserve_0, pool_data.reserve_1)
        )
    }

    #[tokio::test]
    async fn new_from_client() {
        let pair_address: Address = "0x9e02c5ce0bE2029c7a5cD86A37FF463Ff260af51"
            .parse()
            .unwrap();
        let client = Arc::new(get_client());
        let pair_contract = IUniswapV2Pair::new(pair_address, client.clone());
        let token_0 = pair_contract.token_0().await.unwrap();
        let token_1 = pair_contract.token_1().await.unwrap();
        let (reserve_0, reserve_1, _) = pair_contract.get_reserves().await.unwrap();
        let swap_fee = U256::from(3);
        let pool_data = UniswapV2::new_from_client(pair_address, FactoryV2::default(), client)
            .await
            .unwrap();
        assert_eq!(pool_data.token_0, token_0);
        assert_eq!(pool_data.token_1, token_1);
        assert_eq!(pool_data.reserve_0, reserve_0);
        assert_eq!(pool_data.reserve_1, reserve_1);
        assert_eq!(pool_data.block_last_updated, U64::zero());
        assert_eq!(pool_data.factory.swap_fee, swap_fee)
    }

    #[test]
    fn get_amount_out() {
        let pool = create_pool_data();
        let amount_out = pool
            .get_amount_out::<PlaceholderMiddleware>(
                U256::from(10),
                pool.token_0,
                pool.token_1,
                None,
            )
            .unwrap();
        assert_eq!(amount_out, U256::from(987158));

        let amount_out = pool
            .get_amount_out::<PlaceholderMiddleware>(
                U256::from(10),
                pool.token_1,
                pool.token_0,
                None,
            )
            .unwrap();
        assert_eq!(amount_out, U256::zero())
    }
}
