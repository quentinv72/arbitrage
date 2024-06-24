// Targeted exchange types: UNI-V2, UNI-V3
// Max amount in: 1 ETHER
// Path type: WETH ==> Token_A ==> WETH
// Tip percentage: 90%
// New pools not fully supported yet

use std::sync::Arc;

use ethers::middleware::Middleware;
use ethers::prelude::Block;
use ethers::providers::StreamExt;
use ethers::types::{TxHash, U256, U64};
use ethers::utils::WEI_IN_ETHER;
use log::info;
use once_cell::sync::Lazy;
use revm::db::EthersDB;

use pools_graph::arbitrage::arb_paths::ArbPaths;
use pools_graph::arbitrage::arb_tx_v1::ArbTxV1;
use pools_graph::arbitrage::arbs::{ArbPool, Arbs};
use pools_graph::arbitrage::executor::Executor;
use pools_graph::pool_data::factory::{FactoryV2, FactoryV3};
use pools_graph::pool_data::pool_data::PoolData;
use pools_graph::pool_data::uniswap_v2::utils::{CRO_DEFI_FACTORY, load_uniswap_v2_pairs, LUA_SWAP_FACTORY, PANCAKE_SWAP_FACTORY, SUSHISWAP_FACTORY, UNISWAP_V2_FACTORY, ZEUS_FACTORY};
use pools_graph::pool_data::uniswap_v3::utils::{
    load_uniswap_v3_pools, LoadQuoterV3, PANCAKE_SWAP_V3, SUSHI_SWAP_V3, UNISWAP_V3_FACTORY,
};
use pools_graph::pool_data::utils::EthersCacheDB;
use pools_graph::pools_graph::PoolsGraph;
use utils::logging::setup_logging;
use utils::utils::{FlashbotsProvider, Setup, Utils};

static V2_FACTORIES: [&Lazy<FactoryV2>; 5] = [
    // &UNISWAP_V2_FACTORY,
    &SUSHISWAP_FACTORY,
    &LUA_SWAP_FACTORY,
    &CRO_DEFI_FACTORY,
    &ZEUS_FACTORY,
    &PANCAKE_SWAP_FACTORY,
];

static V3_FACTORIES: [&Lazy<FactoryV3>; 2] =
    [&SUSHI_SWAP_V3, &PANCAKE_SWAP_V3];

const APP_NAME: &str = env!("CARGO_CRATE_NAME");

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

const TIP_PERCENTAGE: u32 = 90_00;

const MAX_AMOUNT_IN: U256 = WEI_IN_ETHER;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut first_block = true;
    let utils: Utils<FlashbotsProvider> = Utils::setup().await;
    setup_logging(utils.is_production(), APP_NAME);
    let pools_graph = Arc::new(PoolsGraph::default());
    let v2_factories = V2_FACTORIES
        .iter()
        .map(|x| *Lazy::force(x))
        .collect::<Vec<_>>();
    let v3_factories = V3_FACTORIES
        .iter()
        .map(|x| *Lazy::force(x))
        .collect::<Vec<_>>();
    load_uniswap_v2_pairs(&pools_graph, v2_factories, Arc::clone(&utils.rpc_client)).await?;
    load_uniswap_v3_pools(&pools_graph, v3_factories, Arc::clone(&utils.rpc_client)).await?;
    let arbitrage_paths = init_arb_paths(&pools_graph);
    let all_paths = arbitrage_paths
        .get_all_pools()
        .iter()
        .map(|x| **x)
        .collect::<Vec<_>>();
    let cache_db = EthersCacheDB::new(EthersDB::new(Arc::clone(&utils.rpc_client), None).unwrap());

    let executor = Executor {
        client: Arc::clone(&utils.rpc_client),
        executor_address: utils.bundle_executor,
        sender: utils.sender,
        output_token: WETH.parse().unwrap(),
        chain_id: U64::one(),
        tip_percentage: TIP_PERCENTAGE,
        coinbase_threshold: U256::from_dec_str("34000000000000000").unwrap(),
    };

    let mut arbs: Arbs<FlashbotsProvider, ArbTxV1, _> =
        Arbs::new(arbitrage_paths, cache_db, executor);
    let mut stream = utils.ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {
        info!("Block # {}", block.number.unwrap());
        let block = utils.rpc_client.get_block_with_txs(block.number.unwrap()).await?.unwrap();
        // // info!("Block {block:#?}");
        //  for tx in block.transactions {
        //      let tx_receipt = utils.rpc_client
        //          .get_transaction_receipt(tx.hash)
        //          .await?
        //          .unwrap_or_default();
        //      info!("These are the receipts {tx_receipt:?}");
        //  }
        arbs.load_uniswap_v3_quoter();
        let updated_pools = pools_graph
            .maybe_update_graph(Block::<TxHash>::from(block.clone()), Arc::clone(&utils.rpc_client))
            .await?;
        println!("{updated_pools:#?}");
        // if first_block {
        //     arbs.compute_all_arbitrages(&all_paths, &pools_graph, MAX_AMOUNT_IN, U256::from(50));
        //     first_block = false;
        // } else {
            arbs.compute_all_arbitrages(
                &updated_pools,
                &pools_graph,
                MAX_AMOUNT_IN,
                U256::from(100),
            );
        // }
        // TODO figure out whether nonce management is actually an issue
        arbs.submit_txs(pools_graph.clone(), block).await?;
        arbs.clear_cache();
    }
    Ok(())
}

fn init_arb_paths(pools_graph: &PoolsGraph) -> ArbPaths {
    let mut arb_paths = ArbPaths::default();
    let weth_token_nbd = pools_graph
        .get_neighbouring_tokens(&WETH.parse().unwrap())
        .unwrap();
    for neighbour in weth_token_nbd.iter() {
        for pool in pools_graph
            .get_pool_addresses(WETH.parse().unwrap(), *neighbour)
            .unwrap()
            .value()
            .iter()
        {
            let input_arb_pool = ArbPool {
                pool: *pool,
                token_in: WETH.parse().unwrap(),
                token_out: *neighbour,
            };
            for other_pool in pools_graph
                .get_pool_addresses(WETH.parse().unwrap(), *neighbour)
                .unwrap()
                .value()
                .iter()
            {
                if *other_pool == *pool {
                    continue;
                };
                let output_arb_pool = ArbPool {
                    pool: *other_pool,
                    token_in: *neighbour,
                    token_out: WETH.parse().unwrap(),
                };

                match (
                    pools_graph
                        .get_pool_data(&input_arb_pool.pool)
                        .unwrap()
                        .value(),
                    pools_graph
                        .get_pool_data(&output_arb_pool.pool)
                        .unwrap()
                        .value(),
                ) {
                    (PoolData::UniswapV3(_), _) => {
                        arb_paths
                            .insert_path(vec![input_arb_pool, output_arb_pool])
                            .expect("If this fails then there is an implementation problem.");
                    }
                    (_, PoolData::UniswapV3(_)) => {
                        arb_paths
                            .insert_path(vec![input_arb_pool, output_arb_pool])
                            .expect("If this fails then there is an implementation problem.");
                    }
                    (_, _) => (),
                }
            }
        }
    }

    arb_paths
}
