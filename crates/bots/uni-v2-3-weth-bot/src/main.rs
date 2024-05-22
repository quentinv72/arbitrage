use std::cmp::max;
use std::collections::HashSet;
use std::ops::{Div, Mul};
use std::sync::Arc;

use ethers::prelude::{ContractError, Lazy};
use ethers::providers::{Middleware, StreamExt};
use ethers::types::{Address, U256, U64};
use log::{info};
use pools_graph::pool_data::pool_data::{PoolData, PoolDataTrait};
use pools_graph::pools_graph::PoolsGraph;
use pools_graph::utils::arbitrage::Arbitrage;
use pools_graph::utils::uniswap_v2;
use pools_graph::utils::uniswap_v2::{
    CRO_DEFI_FACTORY, Factory, LUA_SWAP_FACTORY, PANCAKE_SWAP_FACTORY, SUSHISWAP_FACTORY,
    UNISWAP_V2_FACTORY, ZEUS_FACTORY,
};
use rayon::prelude::*;
use utils::logging::setup_logging;
use utils::TOKEN_BLACKLIST;
use utils::utils::{Setup, Utils};

static V2_FACTORIES: [&Lazy<Factory>; 6] = [
    &UNISWAP_V2_FACTORY,
    &SUSHISWAP_FACTORY,
    &LUA_SWAP_FACTORY,
    &CRO_DEFI_FACTORY,
    &ZEUS_FACTORY,
    &PANCAKE_SWAP_FACTORY,
];

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

const APP_NAME: &str = env!("CARGO_CRATE_NAME");

const NUMBER_OF_STEPS: u32 = 1000;

const PRIORITY_FEE_PERCENTAGE: u32 = 99_99;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let utils = Utils::setup().await;
    setup_logging(utils.is_production(), APP_NAME);
    let rpc_client = utils.get_rpc_client();
    let factories = V2_FACTORIES
        .iter()
        .map(|x| Lazy::force(x))
        .collect::<Vec<_>>();
    let graph = Arc::new(PoolsGraph::new());
    uniswap_v2::load_uniswap_v2_pairs(&graph, factories, Arc::clone(&rpc_client)).await?;
    let all_paths = get_all_paths(&graph);
    let mut has_reverted: HashSet<Vec<Address>> = HashSet::new();
    info!("There are {} paths", all_paths.len());
    let ws_client = utils.get_ws_client();
    let mut stream = ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {
        let block_number = block.number.unwrap();
        if block_number != rpc_client.get_block_number().await? {
            continue;
        }
        let next_base_fee = block.next_block_base_fee().unwrap();
        info!("Block number: {block_number} with base fee {next_base_fee}");
        update_reserves(
            block_number,
            &all_paths,
            Arc::clone(&graph),
            Arc::clone(&rpc_client),
        )
        .await?;

        let mut profitable_trades = all_paths
            .par_iter()
            .map(|x| try_finding_arbitrage(&graph, x))
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();
        profitable_trades.sort();
        while let Some(top_item) = profitable_trades.pop() {
            
            top_item
                .submit_transaction_flashbots(
                    &graph,
                    WETH.parse()?,
                    utils.get_bundle_executor_address(),
                    next_base_fee,
                    &mut has_reverted,
                    Arc::clone(&rpc_client),
                    PRIORITY_FEE_PERCENTAGE,
                    block_number,
                )
                .await?;
        }
    }
    Ok(())
}

fn get_all_paths(graph: &PoolsGraph) -> Vec<Path> {
    let mut results = Vec::new();
    let input_tokens = graph
        .get_neighbouring_tokens(&WETH.parse().unwrap())
        .unwrap();
    for input_token in input_tokens.value().iter() {
        if TOKEN_BLACKLIST.contains(&input_token) {
            continue;
        }
        let weth_output_pools = graph
            .get_pool_addresses(*input_token, WETH.parse().unwrap())
            .unwrap()
            .value()
            .iter()
            .map(|x| *x)
            .collect::<Vec<Address>>();
        let output_tokens = graph.get_neighbouring_tokens(&input_token.clone()).unwrap();
        for output_token in output_tokens.value().iter() {
            if output_token.0 == WETH.parse::<Address>().unwrap().0 {
                continue;
            }
            let all_pools = graph
                .get_pool_addresses(*input_token, *output_token)
                .unwrap();
            for input_pool_address in all_pools.value().iter() {
                for output_pool_address in all_pools.iter() {
                    if output_pool_address.0 != input_pool_address.0 {
                        results.push(Path {
                            input_token: *input_token,
                            input_pool_address: *input_pool_address,
                            output_pool_address: *output_pool_address,
                            weth_output_pools: weth_output_pools.clone(),
                        })
                    }
                }
            }
        }
    }
    results
}

async fn update_reserves<M: Middleware>(
    current_block: U64,
    paths: &Vec<Path>,
    pools_graph: Arc<PoolsGraph>,
    client: Arc<M>,
) -> Result<(), ContractError<M>> {
    for path in paths {
        uniswap_v2::refresh_reserves(
            &pools_graph,
            &path.input_pool_address,
            current_block,
            client.clone(),
        )
        .await?;
        uniswap_v2::refresh_reserves(
            &pools_graph,
            &path.output_pool_address,
            current_block,
            client.clone(),
        )
        .await?;
        for pool_address in &path.weth_output_pools {
            uniswap_v2::refresh_reserves(
                &pools_graph,
                pool_address,
                current_block,
                client.clone(),
            )
            .await?;
        }
    }
    Ok(())
}

fn try_finding_arbitrage(graph: &PoolsGraph, path: &Path) -> Option<Arbitrage> {
    let (input_token_0, _) = graph
        .get_pool_data(&path.input_pool_address)
        .unwrap()
        .get_tokens();
    let zero_for_one = input_token_0 == path.input_token;
    let (input_reserve_in, input_reserve_out) =
        get_uniswap_v2_pair_reserves(&path.input_pool_address, graph, zero_for_one);
    let (output_reserve_in, output_reserve_out) =
        get_uniswap_v2_pair_reserves(&path.output_pool_address, graph, !zero_for_one);
    let max_amount_in = max_amount_in(
        input_reserve_in,
        input_reserve_out,
        output_reserve_in,
        output_reserve_out,
    );
    if max_amount_in > U256::zero() {
        return calculate_profit(graph, path, max_amount_in, zero_for_one);
    }
    None
}

fn calculate_profit(
    graph: &PoolsGraph,
    path: &Path,
    max_amount_in: U256,
    zero_for_one: bool,
) -> Option<Arbitrage> {
    let mut i = U256::one();
    let step_size = max(U256::one(), max_amount_in.div(U256::from(NUMBER_OF_STEPS)));
    let mut profit = U256::zero();
    let mut amount_in_first = U256::zero();
    let mut amount_out_first = U256::zero();
    let mut amount_out_second = U256::zero();
    let mut amount_out_third = U256::zero();
    let mut output_address = Address::zero();
    let mut zero_for_one_weth = true;
    let input_pool = graph.get_pool_data(&path.input_pool_address).unwrap();
    let output_pool = graph.get_pool_data(&path.output_pool_address).unwrap();
    while i < max_amount_in {
        let a = input_pool.get_amount_out(i, zero_for_one);
        let b = output_pool.get_amount_out(a, !zero_for_one);
        if b > i && b - i > profit {
            amount_in_first = i;
            amount_out_first = a;
            amount_out_second = b;
            profit = b - i;
        }
        i += step_size;
    }
    for weth_pool_address in &path.weth_output_pools {
        let weth_pool = graph.get_pool_data(weth_pool_address).unwrap();
        let (t0, _) = weth_pool.get_tokens();
        let _zero_for_one_weth = t0 == path.input_token;
        let out = weth_pool.get_amount_out(profit, _zero_for_one_weth);
        if out > amount_out_third {
            amount_out_third = out;
            output_address = *weth_pool_address;
            zero_for_one_weth = _zero_for_one_weth;
        }
    }
    if amount_out_third > U256::zero() {
        return Some(Arbitrage::new(
            vec![
                path.input_pool_address,
                path.output_pool_address,
                output_address,
            ],
            vec![amount_in_first, amount_out_first, profit],
            vec![amount_out_first, amount_out_second, amount_out_third],
            vec![zero_for_one, !zero_for_one, zero_for_one_weth],
            U256::zero(),
            Some(amount_out_third),
        ));
    }

    None
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
    let c = U256::from(997).mul(input_reserve_out) + U256::from(1_000).mul(output_reserve_in);

    if a < b || c.is_zero() {
        return U256::zero();
    }

    let numerator = a - b;

    let denominator = U256::from(997).mul(c);
    numerator.div(denominator)
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

#[derive(Debug)]
struct Path {
    input_token: Address,
    input_pool_address: Address,
    output_pool_address: Address,
    weth_output_pools: Vec<Address>,
}
