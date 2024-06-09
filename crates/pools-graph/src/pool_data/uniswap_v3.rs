use std::sync::Arc;

use anyhow::anyhow;
use ethers::abi::{AbiDecode, AbiEncode, Error, RawLog};
use ethers::contract::{ContractError, EthEvent};
use ethers::middleware::Middleware;
use ethers::prelude::{H256, U64};
use ethers::types::{Address, Bytes, U256};
use revm::primitives::{address, ruint, ExecutionResult, TransactTo};
use revm::Evm;

use contracts::i_uniswap_v_3_factory::PoolCreatedFilter;
use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;
use contracts::uniswap_v_3_quoter::GetAmountOutCall;

use crate::pool_data::factory::{Factory, FactoryV3};
use crate::pool_data::pool_data::PoolDataTrait;
use crate::utils::EthersCacheDB;

#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct UniswapV3 {
    pool_address: Address,
    sqrt_price_x_96: U256,
    token_0: Address,
    token_1: Address,
    fee_tier: u32,
    block_last_updates: U64,
    factory: FactoryV3,
}

impl UniswapV3 {
    pub async fn new_from_client<M: Middleware>(
        pool_address: Address,
        token_0: Address,
        token_1: Address,
        fee_tier: u32,
        factory: FactoryV3,
        client: Arc<M>,
    ) -> Result<Self, ContractError<M>> {
        let pool_contract = IUniswapV3Pool::new(pool_address, client);
        let (sqrt_price_x_96, _, _, _, _, _, _) = pool_contract.slot_0().call().await?;
        Ok(Self {
            pool_address,
            sqrt_price_x_96,
            token_0,
            token_1,
            fee_tier,
            block_last_updates: U64::zero(),
            factory,
        })
    }

    pub(crate) fn get_new_pool_sig() -> H256 {
        PoolCreatedFilter::signature()
    }

    pub(crate) fn new_from_log(log: &RawLog, factory_v3: FactoryV3) -> Result<Self, Error> {
        let parsed_event = PoolCreatedFilter::decode_log(log)?;
        Ok(Self {
            pool_address: parsed_event.pool,
            token_1: parsed_event.token_1,
            token_0: parsed_event.token_0,
            fee_tier: parsed_event.fee,
            factory: factory_v3,
            ..Default::default()
        })
    }

    // Only used in tests
    pub fn new(
        pool_address: Address,
        sqrt_price_x_96: U256,
        token_0: Address,
        token_1: Address,
        fee_tier: u32,
        factory: Option<FactoryV3>,
    ) -> Self {
        Self {
            pool_address,
            sqrt_price_x_96,
            token_0,
            token_1,
            fee_tier,
            block_last_updates: U64::zero(),
            factory: factory.unwrap_or_default(),
        }
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

    fn get_factory(&self) -> Factory {
        self.factory.into()
    }

    #[inline]
    fn get_amount_out<M: Middleware>(
        &self,
        amount_in: U256,
        zero_for_one: bool,
        cache_db: Option<&mut EthersCacheDB<M>>,
    ) -> anyhow::Result<U256> {
        let cache_db = cache_db.expect("Missing cache db");
        let encoded = GetAmountOutCall {
            pool: self.pool_address,
            zero_for_one,
            amount_in,
        }
        .encode();

        let mut evm = Evm::builder()
            .with_db(cache_db)
            .modify_tx_env(|tx| {
                // 0x1 because calling USDC proxy from zero address fails
                tx.caller = address!("0000000000000000000000000000000000000001");
                tx.transact_to =
                    TransactTo::Call(address!("0000000000000000000000000000000000000004"));
                tx.data = encoded.into();
                tx.value = ruint::aliases::U256::from(0);
            })
            .build();

        let ref_tx = evm.transact().unwrap();
        let result = ref_tx.result;
        let output = match result {
            ExecutionResult::Revert { output, .. } => <(U256, U256)>::decode(output)?,
            result => {
                return Err(anyhow!(
                    "UniswapV3::get_amount_out execution failed: {result:#?}"
                ))
            }
        };

        if zero_for_one {
            Ok(output.1)
        } else {
            Ok(output.0)
        }
    }

    fn build_swap_calldata(
        &self,
        _amount_in: U256,
        _amount_out: U256,
        _zero_for_one: bool,
        _data: Bytes,
        _bundle_executor_address: Address,
    ) -> Bytes {
        todo!("V3 swap calldata not implemented")
    }

    async fn update_pool<M: Middleware>(
        &mut self,
        _client: Arc<M>,
    ) -> Result<(), ContractError<M>> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ethers::prelude::{Http, Provider};
    use ethers::types::{Address, U64};

    use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;

    use crate::pool_data::factory::FactoryV3;
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
        let token_0 = Address::random();
        let token_1 = Address::random();
        let fee_tier = 5;
        let (sqrt_price_x_96, _, _, _, _, _, _) = pool_contract.slot_0().await.unwrap();
        let pool_data = UniswapV3::new_from_client(
            pool_address,
            token_0,
            token_1,
            fee_tier,
            FactoryV3::default(),
            client,
        )
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
