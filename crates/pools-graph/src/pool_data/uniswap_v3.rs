use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use ethers::abi::{AbiDecode, AbiEncode, Error, RawLog};
use ethers::contract::{ContractError, EthEvent};
use ethers::middleware::Middleware;
use ethers::prelude::{H256, U64};
use ethers::types::{Address, Bytes, I256, U256};
use revm::Evm;
use revm::primitives::{
    AccountInfo, address, alloy_primitives, Bytecode, ExecutionResult, Output, ruint, TransactTo,
};

use contracts::i_uniswap_v_3_factory::PoolCreatedFilter;
use contracts::i_uniswap_v_3_pool::IUniswapV3Pool;
use contracts::uniswap_v_3_quoter::GetAmountOutCall;

use crate::pool_data::factory::{Factory, FactoryV3};
use crate::pool_data::pool_data::PoolDataTrait;
use crate::utils::EthersCacheDB;

// TODO: determine what value should be used for..
const QUOTER_MOCK_ADDRESS: alloy_primitives::Address =
    address!("1F98431c8aD98523631AE4a59f267346ea31F984");

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

    pub fn load_quoter_bytecode<M: Middleware>(cache_db: &mut EthersCacheDB<M>) {
        // bytecode generated from UniswapV3Quoter.sol
        let string_bytecode = "0x60806040526004361061002c575f3560e01c8063fa461e3314610037578063fba21b3d1461005f57610033565b3661003357005b5f80fd5b348015610042575f80fd5b5061005d60048036038101906100589190610257565b61009c565b005b34801561006a575f80fd5b506100856004803603810190610080919061038a565b6100ad565b6040516100939291906103e9565b60405180910390f35b604051848152836020820152604081fd5b5f805f846100cf5773fffd8963efd1fc6a506488495d951d5263988d256100d6565b6401000276ad5b90508573ffffffffffffffffffffffffffffffffffffffff1663128acb0860018787856040518563ffffffff1660e01b81526004016101189493929190610470565b60408051808303815f875af192505050801561015257506040513d601f19601f8201168201806040525081019061014f91906104da565b60015b6101af573d805f8114610180576040519150601f19603f3d011682016040523d82523d5f602084013e610185565b606091505b505f808280602001905181019061019c91906104da565b91509150818195509550505050506101b3565b5050505b935093915050565b5f80fd5b5f80fd5b5f819050919050565b6101d5816101c3565b81146101df575f80fd5b50565b5f813590506101f0816101cc565b92915050565b5f80fd5b5f80fd5b5f80fd5b5f8083601f840112610217576102166101f6565b5b8235905067ffffffffffffffff811115610234576102336101fa565b5b6020830191508360018202830111156102505761024f6101fe565b5b9250929050565b5f805f806060858703121561026f5761026e6101bb565b5b5f61027c878288016101e2565b945050602061028d878288016101e2565b935050604085013567ffffffffffffffff8111156102ae576102ad6101bf565b5b6102ba87828801610202565b925092505092959194509250565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6102f1826102c8565b9050919050565b610301816102e7565b811461030b575f80fd5b50565b5f8135905061031c816102f8565b92915050565b5f8115159050919050565b61033681610322565b8114610340575f80fd5b50565b5f813590506103518161032d565b92915050565b5f819050919050565b61036981610357565b8114610373575f80fd5b50565b5f8135905061038481610360565b92915050565b5f805f606084860312156103a1576103a06101bb565b5b5f6103ae8682870161030e565b93505060206103bf86828701610343565b92505060406103d086828701610376565b9150509250925092565b6103e3816101c3565b82525050565b5f6040820190506103fc5f8301856103da565b61040960208301846103da565b9392505050565b610419816102e7565b82525050565b61042881610322565b82525050565b610437816102c8565b82525050565b5f82825260208201905092915050565b50565b5f61045b5f8361043d565b91506104668261044d565b5f82019050919050565b5f60a0820190506104835f830187610410565b610490602083018661041f565b61049d60408301856103da565b6104aa606083018461042e565b81810360808301526104bb81610450565b905095945050505050565b5f815190506104d4816101cc565b92915050565b5f80604083850312156104f0576104ef6101bb565b5b5f6104fd858286016104c6565b925050602061050e858286016104c6565b915050925092905056fea26469706673582212201710a522af1d5f5f9f4ec95b24cbb396a4ca37fe2a24a0bb5f7ed99a301550d564736f6c634300081a0033";
        let bytes = alloy_primitives::Bytes::from_str(string_bytecode)
            .unwrap();
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
        let anvil = Anvil::new()
            .fork("https://eth-sepolia.g.alchemy.com/v2/fEmCuDGqB-tSA4R5HnnVCy1n9Jg4GqJg@6077409")
            .spawn();

        let provider = Arc::new(Provider::<Http>::try_from(anvil.endpoint()).unwrap());

        let pool = UniswapV3 {
            pool_address: "0x9799b5EDC1aA7D3FAd350309B08df3F64914E244"
                .parse()
                .unwrap(),
            sqrt_price_x_96: U256::zero(),
            token_0: "0x94a9D9AC8a22534E3FaCa9F4e7F2E2cf85d5E4C8"
                .parse()
                .unwrap(),
            token_1: "0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14"
                .parse()
                .unwrap(),
            fee_tier: 3,
            block_last_updates: U64::zero(),
            factory: Default::default(),
        };

        let ethers_db = EthersDB::new(provider, None).unwrap();

        let mut cache_db = CacheDB::new(ethers_db);

        UniswapV3::load_quoter_bytecode(&mut cache_db);
        let amount_out = pool
            .get_amount_out(U256::from(1000), true, Some(&mut cache_db))
            .unwrap();
        assert_eq!(amount_out, U256::from(455741850));

        let amount_out = pool
            .get_amount_out(U256::from(10), true, Some(&mut cache_db))
            .unwrap();
        assert_eq!(amount_out, U256::from(4114018));

        let amount_out = pool
            .get_amount_out(U256::from(100000000), false, Some(&mut cache_db))
            .unwrap();
        assert_eq!(amount_out, U256::from(218));

        drop(anvil);
    }
}
