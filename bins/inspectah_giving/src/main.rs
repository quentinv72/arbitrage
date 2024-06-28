// Targeted exchange types: UNI-V2, UNI-V3
// Max amount in: 1 ETHER
// Path type: WETH ==> Token_A ==> WETH
// Tip percentage: 90%
// New pools not fully supported yet

use std::collections::HashSet;
use std::ops::Mul;
use std::sync::Arc;

use ethers::middleware::Middleware;
use ethers::prelude::Block;
use ethers::providers::StreamExt;
use ethers::types::{Address, TxHash, U256, U64};
use ethers::utils::WEI_IN_ETHER;
use itertools::Itertools;
use log::{debug, info};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use revm::db::EthersDB;

use pools_graph::arbitrage::arb_paths::{ArbPaths, ArbPathsErrors};
use pools_graph::arbitrage::arb_tx_v1::ArbTxV1;
use pools_graph::arbitrage::arbs::{ArbPool, Arbs};
use pools_graph::arbitrage::executor::Executor;
use pools_graph::pool_data::factory::{FactoryV2, FactoryV3};
use pools_graph::pool_data::pool_data::PoolData;
use pools_graph::pool_data::uniswap_v2::utils::{
    load_uniswap_v2_pairs, CRO_DEFI_FACTORY, LUA_SWAP_FACTORY, PANCAKE_SWAP_FACTORY,
    SUSHISWAP_FACTORY, UNISWAP_V2_FACTORY, ZEUS_FACTORY,
};
use pools_graph::pool_data::uniswap_v3::utils::{
    load_uniswap_v3_pools, LoadQuoterV3, PANCAKE_SWAP_V3, SUSHI_SWAP_V3, UNISWAP_V3_FACTORY,
};
use pools_graph::pool_data::utils::EthersCacheDB;
use pools_graph::pools_graph::PoolsGraph;
use utils::logging::setup_logging;
use utils::utils::{FlashbotsProvider, Setup, Utils};

static V2_FACTORIES: [&Lazy<FactoryV2>; 2] = [
    &UNISWAP_V2_FACTORY,
    &SUSHISWAP_FACTORY,
    // &LUA_SWAP_FACTORY,
    // &CRO_DEFI_FACTORY,
    // &ZEUS_FACTORY,
    // &PANCAKE_SWAP_FACTORY,
];

static V3_FACTORIES: [&Lazy<FactoryV3>; 1] = [&UNISWAP_V3_FACTORY];

const APP_NAME: &str = env!("CARGO_CRATE_NAME");

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

const TIP_PERCENTAGE: u32 = 99_00;

const MAX_AMOUNT_IN: U256 = WEI_IN_ETHER;

const PATH_LENGTH: u8 = 4;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let arbitrage_paths = init_arb_(&pools_graph);
    // println!("{arbitrage_paths:#?}");
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
    let mut arbs: Arbs<FlashbotsProvider, ArbTxV1, _> = Arbs::new(
        arbitrage_paths,
        cache_db,
        executor,
        U256::from_dec_str("1200000000000000").unwrap(),
    );
    let mut stream = utils.ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {
        info!("Block # {}", block.number.unwrap());
        let block = utils
            .rpc_client
            .get_block_with_txs(block.number.unwrap())
            .await?
            .unwrap();
        arbs.load_uniswap_v3_quoter();
        let updated_pools = pools_graph
            .maybe_update_graph(
                Block::<TxHash>::from(block.clone()),
                Arc::clone(&utils.rpc_client),
            )
            .await?;

        arbs.compute_all_arbitrages(&updated_pools, &pools_graph, MAX_AMOUNT_IN, U256::from(30));
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

fn init_arb_(pools_graph: &PoolsGraph) -> ArbPaths {
    let start_tokens = WETH.parse().unwrap();

    let mut arb_path = ArbPaths::default();
    let token_paths = token_paths(start_tokens, pools_graph);
    let paths: Vec<Vec<ArbPool>> = token_paths
        .par_iter()
        .map(|path| {
            // below represents a list of every possible pool for each pair in the
            // path.
            path.windows(2)
                .map(|tokens| {
                    let input_token = tokens[0];
                    let output_token = tokens[1];
                    pools_graph
                        .get_pool_addresses(input_token, output_token)
                        .unwrap()
                        .iter()
                        .map(|_pool| ArbPool {
                            pool: *_pool,
                            token_in: input_token,
                            token_out: output_token,
                        })
                        .collect()
                })
                .collect::<Vec<Vec<ArbPool>>>()
        })
        // Take cartesian product of pools associated with each pair in a token path and filter them.
        .flat_map(|pool_paths| {
            pool_paths
                .iter()
                .multi_cartesian_product()
                .map(|arb_pool_path| arb_pool_path.iter().map(|x| **x).collect())
                .collect::<Vec<_>>()
        })
        .collect();

    for path in paths {
        for pool in &path {
            if let PoolData::UniswapV3(_) = pools_graph.get_pool_data(&pool.pool).unwrap().value() {
                match arb_path.insert_path(path) {
                    Err(ArbPathsErrors::DuplicatedPools) => {}
                    _ => {}
                }
                break;
            }
        }
    }
    arb_path
}

fn token_paths(start_token: Address, pools_graph: &PoolsGraph) -> HashSet<Vec<Address>> {
    let output_token = WETH.parse().unwrap();
    let mut token_paths = HashSet::new();
    let mut stack = Vec::new();
    stack.push((start_token, vec![start_token]));
    while let Some((curr_item, curr_path)) = stack.pop() {
        if curr_path.len() as u8 == PATH_LENGTH {
            token_paths.insert(curr_path);
            continue;
        }
        let curr_item_neighbours = pools_graph.get_neighbouring_tokens(&curr_item).unwrap();
        if curr_path.len() as u8 == (PATH_LENGTH - 1) {
            if curr_item_neighbours.contains(&output_token) {
                let mut new_path = curr_path.clone();
                new_path.push(output_token);
                stack.push((output_token, new_path))
            }
            continue;
        }
        for neighbour in curr_item_neighbours.iter() {
            let mut new_path = curr_path.clone();
            new_path.push(*neighbour);
            stack.push((*neighbour, new_path));
        }
    }

    token_paths
}
