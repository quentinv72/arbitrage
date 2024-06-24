// Targeted exchange types: UNI-V2, UNI-V3
// Max amount in: 1 ETHER
// Path type: WETH ==> Token_A ==> Token_B ==> WETH
// Tip percentage: 90%
// New pools not fully supported yet

use std::sync::Arc;

use ethers::middleware::Middleware;
use ethers::providers::StreamExt;
use ethers::types::{U256, U64};
use once_cell::sync::Lazy;
use revm::db::EthersDB;

use pools_graph::arbitrage::arb_paths::ArbPaths;
use pools_graph::arbitrage::executor::Executor;
use pools_graph::pool_data::factory::{FactoryV2, FactoryV3};
use pools_graph::pool_data::uniswap_v2::utils::{CRO_DEFI_FACTORY, load_uniswap_v2_pairs, LUA_SWAP_FACTORY, PANCAKE_SWAP_FACTORY, SUSHISWAP_FACTORY, UNISWAP_V2_FACTORY, ZEUS_FACTORY};
use pools_graph::pool_data::uniswap_v3::utils::load_uniswap_v3_pools;
use pools_graph::pool_data::utils::EthersCacheDB;
use pools_graph::pools_graph::PoolsGraph;
use utils::logging::setup_logging;
use utils::utils::{FlashbotsProvider, Setup, Utils};

static V2_FACTORIES: [&Lazy<FactoryV2>; 6] = [
    &UNISWAP_V2_FACTORY,
    &SUSHISWAP_FACTORY,
    &LUA_SWAP_FACTORY,
    &CRO_DEFI_FACTORY,
    &ZEUS_FACTORY,
    &PANCAKE_SWAP_FACTORY,
];

static V3_FACTORIES: [&Lazy<FactoryV3>; 0] = [];

const APP_NAME: &str = env!("CARGO_CRATE_NAME");

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

const TIP_PERCENTAGE: u32 = 90_00;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let utils: Utils<FlashbotsProvider> = Utils::setup().await;
    setup_logging(utils.is_production(), APP_NAME);
    let pools_graph = PoolsGraph::default();
    let v2_factories = V2_FACTORIES
        .iter()
        .map(|x| Lazy::force(x))
        .collect::<Vec<_>>();
    load_uniswap_v2_pairs(&pools_graph, v2_factories, Arc::clone(&utils.rpc_client)).await?;
    load_uniswap_v3_pools(&pools_graph, vec![], Arc::clone(&utils.rpc_client)).await?;
    let arbitrage_paths = ArbPaths::default();

    let cache_db = EthersCacheDB::new(EthersDB::new(Arc::clone(&utils.rpc_client), None).unwrap());
    let executor = Executor {
        client: Arc::clone(&utils.rpc_client),
        executor_address: utils.bundle_executor,
        senders: vec![],
        output_token: WETH.parse().unwrap(),
        chain_id: U64::one(),
        tip_percentage: TIP_PERCENTAGE,
        coinbase_threshold: U256::from_dec_str("34000000000000000").unwrap(),
    };

    // let arbs = Arbs::new();
    let mut stream = utils.ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {}
    Ok(())
}
