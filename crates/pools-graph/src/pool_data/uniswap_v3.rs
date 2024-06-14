use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use ethers::abi::{AbiDecode, AbiEncode, Error, RawLog};
use ethers::contract::{ContractError, EthEvent};
use ethers::middleware::Middleware;
use ethers::prelude::{H256, U64};
use ethers::types::{Address, Bytes, I256, U256};
use revm::primitives::{
    address, alloy_primitives, ruint, AccountInfo, Bytecode, ExecutionResult, Output, TransactTo,
};
use revm::Evm;

use contracts::i_uniswap_v_3_factory::PoolCreatedFilter;
use contracts::i_uniswap_v_3_pool::{IUniswapV3Pool, SwapCall};
use contracts::uniswap_v_3_quoter::GetAmountOutCall;

use crate::pool_data::factory::{Factory, FactoryV3};
use crate::pool_data::pool_data::PoolDataTrait;
use crate::utils::EthersCacheDB;

// TODO: determine what value should be used for..
pub(crate) const QUOTER_MOCK_ADDRESS: alloy_primitives::Address =
    address!("1F98431c8aD98523631AE4a59f267346ea31F984");

// bytecode generated from UniswapV3Quoter.sol
pub(crate) const QUOTER_BYTECODE: &str = include_str!("UniswapV3Quoter.hex");

#[derive(Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct UniswapV3 {
    pub pool_address: Address,
    pub sqrt_price_x_96: U256,
    pub token_0: Address,
    pub token_1: Address,
    pub fee_tier: u32,
    pub block_last_updates: U64,
    pub factory: FactoryV3,
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

    pub fn load_quoter_bytecode<M: Middleware>(cache_db: &mut EthersCacheDB<M>) {
        let bytes = alloy_primitives::Bytes::from_str(QUOTER_BYTECODE).unwrap();
        let bytecode = Bytecode::new_raw(bytes);
        cache_db.insert_account_info(
            QUOTER_MOCK_ADDRESS,
            AccountInfo {
                balance: ruint::aliases::U256::from(0),
                nonce: 1,
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode),
            },
        )
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
        token_in: Address,
        _token_out: Address,
        cache_db: Option<&mut EthersCacheDB<M>>,
    ) -> anyhow::Result<U256> {
        let zero_for_one = token_in == self.token_0;
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
                tx.transact_to = TransactTo::Call(QUOTER_MOCK_ADDRESS);
                tx.data = encoded.into();
                tx.value = ruint::aliases::U256::from(0);
            })
            .build();
        let ref_tx = evm.transact().unwrap();
        let result = ref_tx.result;
        let (amount_0_delta, amount_1_delta) = match result {
            ExecutionResult::Success {
                output: Output::Call(value),
                ..
            } => <(I256, I256)>::decode(value)?,
            result => {
                return Err(anyhow!(
                    "UniswapV3::get_amount_out execution failed: {result:#?}"
                ));
            }
        };
        // The amount out will be negative and amount in is positive
        let min = -amount_0_delta.min(amount_1_delta);
        Ok(min.into_raw())
    }

    #[inline]
    fn build_swap_calldata(
        &self,
        amount_in: U256,
        _amount_out: U256,
        token_in: Address,
        _token_out: Address,
        data: Bytes,
        bundle_executor_address: Address,
    ) -> Bytes {
        let zero_for_one = token_in == self.token_0;
        let sqrt_price_limit = if zero_for_one {
            U256::from_dec_str("4295128749").unwrap()
        } else {
            U256::from_dec_str("1461446703485210103287273052203988822378723970341").unwrap()
        };

        SwapCall {
            recipient: bundle_executor_address,
            zero_for_one,
            amount_specified: I256::try_from(amount_in).unwrap(),
            sqrt_price_limit_x96: sqrt_price_limit,
            data,
        }
        .encode()
        .into()
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
    use ethers::types::{Address, U256, U64};
    use ethers::utils::Anvil;
    use revm::db::{CacheDB, EthersDB};

    use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;

    use crate::pool_data::factory::FactoryV3;
    use crate::pool_data::pool_data::PoolDataTrait;
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

    #[tokio::test]
    async fn get_amount_out() {
        let token_0 = "0x94a9D9AC8a22534E3FaCa9F4e7F2E2cf85d5E4C8"
            .parse()
            .unwrap();
        let token_1 = "0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14"
            .parse()
            .unwrap();
        let anvil = Anvil::new()
            .fork("https://eth-sepolia.g.alchemy.com/v2/fEmCuDGqB-tSA4R5HnnVCy1n9Jg4GqJg@6077409")
            .spawn();

        let provider = Arc::new(Provider::<Http>::try_from(anvil.endpoint()).unwrap());

        let pool = UniswapV3 {
            pool_address: "0x9799b5EDC1aA7D3FAd350309B08df3F64914E244"
                .parse()
                .unwrap(),
            sqrt_price_x_96: U256::zero(),
            token_0,
            token_1,
            fee_tier: 3,
            block_last_updates: U64::zero(),
            factory: Default::default(),
        };

        let ethers_db = EthersDB::new(provider, None).unwrap();

        let mut cache_db = CacheDB::new(ethers_db);

        UniswapV3::load_quoter_bytecode(&mut cache_db);
        let amount_out = pool
            .get_amount_out(U256::from(1000), token_0, token_1, Some(&mut cache_db))
            .unwrap();
        assert_eq!(amount_out, U256::from(455741850));

        let amount_out = pool
            .get_amount_out(U256::from(10), token_0, token_1, Some(&mut cache_db))
            .unwrap();
        assert_eq!(amount_out, U256::from(4114018));

        let amount_out = pool
            .get_amount_out(U256::from(100000000), token_1, token_0, Some(&mut cache_db))
            .unwrap();
        assert_eq!(amount_out, U256::from(218));

        drop(anvil);
    }
}
