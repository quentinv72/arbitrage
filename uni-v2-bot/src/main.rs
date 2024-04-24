use std::cmp::max;
use std::collections::HashSet;
use std::ops::{Div, Mul};
use std::sync::Arc;

use ethers::abi::Hash;
use ethers::middleware::Middleware;
use ethers::prelude::ContractError;
use ethers::providers::StreamExt;
use ethers::types::{Address, U256, U64};
use log::{error, info, warn};
use pools_graph::pool_data::pool_data::{PoolData, PoolDataTrait};
use pools_graph::pool_data::uniswap_v2::UniswapV2;
use pools_graph::pools_graph::PoolsGraph;
use pools_graph::utils::uniswap_v2;
use tokio::task::JoinSet;
use tokio::time::Instant;
use utils::logging::setup_logging;
use utils::utils::{Setup, Utils};

const UNISWAP_V2_FACTORIES: [&str; 5] = [
    // "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
    "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac",
    "0x0388c1e0f210abae597b7de712b9510c6c36c857",
    "0x9DEB29c9a4c7A88a3C0257393b7f3335338D9A9D",
    "0xbdda21dd8da31d5bee0c9bb886c044ebb9b8906a",
    "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362",
];

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

// There is probably some env variable that I can use here...
const APP_NAME: &str = "uni_v2_bot";

const STEP_SIZE: u32 = 10000;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let utils = Utils::setup().await;
    setup_logging(utils.is_production(), APP_NAME);
    let factories = UNISWAP_V2_FACTORIES
        .map(|x| x.parse::<Address>().unwrap())
        .to_vec();
    let graph = Arc::new(PoolsGraph::new());
    uniswap_v2::load_uniswap_v2_pairs(&graph, factories, utils.get_rpc_client()).await?;
    let paths = get_paths_2(&graph, WETH.parse()?);
    info!("Found {} paths", paths.len());
    let ws_client = utils.get_ws_client();
    let mut stream = ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {
        let block_number = block.number.unwrap();
        info!("Block number: {}", block_number);
        update_reserves(block_number, &paths, graph.clone(), utils.get_rpc_client()).await?;
        for path in &paths {
            let zero_for_one = path.0 == WETH.parse()?;
            let (input_reserve_in, input_reserve_out) =
                get_uniswap_v2_pair_reserves(&path.0, &graph, zero_for_one);
            let (output_reserve_in, output_reserve_out) =
                get_uniswap_v2_pair_reserves(&path.1, &graph, zero_for_one);
            let max_amount_in = max_amount_in(
                input_reserve_in,
                input_reserve_out,
                output_reserve_in,
                output_reserve_out,
            );
            if max_amount_in > U256::zero() {
                let (amount_in_first, amount_out_first, amount_out_second, profit) =
                    calculate_profit(&graph, &path.0, &path.1, max_amount_in, zero_for_one);
                if profit > U256::zero() {
                    Arbitrage::info!("Profit of {profit} wei");
                }
            }
        }
    }

    let client = utils.get_rpc_client();

    Ok(())
}

// Finds all paths of size 2 starting with a given token and outputting the same token
// A path is exchange_a_addr -> exchange_b_addr, where exchange_a and exchange_b have the
// pairs (start_token/other).
fn get_paths_2(pools_graph: &PoolsGraph, start_token: Address) -> Vec<(Address, Address)> {
    let tokens = pools_graph
        .get_neighbouring_tokens(&start_token)
        .expect("WETH token should have neighbours...");
    let mut results = Vec::new();
    for token in tokens.iter() {
        let exchanges = pools_graph
            .get_pool_addresses(start_token, *token)
            .expect("Pair should have at least one exchange");
        if exchanges.len() == 1 {
            continue;
        }
        for pair_addr in exchanges.iter() {
            for pair_other_addr in exchanges.iter() {
                if *pair_addr != *pair_other_addr {
                    results.push((Address::from(pair_addr.0), Address::from(pair_other_addr.0)))
                }
            }
        }
    }
    results
}

async fn update_reserves<M: Middleware>(
    current_block: U64,
    paths: &Vec<(Address, Address)>,
    pools_graph: Arc<PoolsGraph>,
    client: Arc<M>,
) -> Result<(), ContractError<M>> {
    for path in paths {
        let input_address = path.0;
        let output_address = path.1;
        uniswap_v2::refresh_reserves(&pools_graph, &input_address, current_block, client.clone())
            .await?;
        uniswap_v2::refresh_reserves(&pools_graph, &output_address, current_block, client.clone())
            .await?;
    }
    Ok(())
}

fn get_uniswap_v2_pair_reserves(
    pair_address: &Address,
    pools_graph: &PoolsGraph,
    zero_for_one: bool,
) -> (U256, U256) {
    match pools_graph.get_pool_data(pair_address).unwrap().value() {
        PoolData::UniswapV2(inner) => {
            let (reserve_0, reserve_1) = inner.get_reserves();
            if zero_for_one {
                (U256::from(reserve_0), U256::from(reserve_1))
            } else {
                (U256::from(reserve_1), U256::from(reserve_0))
            }
        }
        _other => panic!("Should be a uniswap V2 pair"),
    }
}

fn has_arbitrage(
    input_reserve_in: U256,
    input_reserve_out: U256,
    output_reserve_in: U256,
    output_reserve_out: U256,
) -> bool {
    let upper_bound = U256::from(994009)
        .mul(input_reserve_out)
        .mul(output_reserve_out)
        .div(U256::from(1000000).mul(input_reserve_in));
    output_reserve_in < upper_bound
}

fn max_amount_in(
    input_reserve_in: U256,
    input_reserve_out: U256,
    output_reserve_in: U256,
    output_reserve_out: U256,
) -> U256 {
    let a = U256::from(994_009)
        .mul(input_reserve_out)
        .mul(output_reserve_out);
    let b = U256::from(1_000_000)
        .mul(input_reserve_in)
        .mul(output_reserve_in);
    if a < b {
        return U256::zero();
    }
    let numerator = a - b;
    let c = U256::from(997).mul(input_reserve_out) + U256::from(1_000).mul(output_reserve_in);
    let denominator = U256::from(997).mul(c);
    numerator.div(denominator)
}

fn calculate_profit(
    pools_graph: &PoolsGraph,
    input_pair_address: &Address,
    output_pair_address: &Address,
    max_amount_in: U256,
    zero_for_one: bool,
) -> (U256, U256, U256, U256) {
    let mut i = U256::one();
    let step_size = max_amount_in.div(U256::from(STEP_SIZE));
    let mut profit = U256::zero();
    let mut amount_in_first = U256::zero();
    let mut amount_out_first = U256::zero();
    let mut amount_out_second = U256::zero();
    let input_pair = pools_graph.get_pool_data(input_pair_address).unwrap();
    let output_pair = pools_graph.get_pool_data(output_pair_address).unwrap();
    while i < max_amount_in {
        let a = input_pair.get_amount_out(i, zero_for_one);
        let b = output_pair.get_amount_out(a, !zero_for_one);
        if b > i && b - i > profit {
            amount_in_first = i;
            amount_out_first = a;
            amount_out_second = b;
            profit = b - i;
        }
        i += step_size;
    }
    (amount_in_first, amount_out_first, amount_out_second, profit)
}
