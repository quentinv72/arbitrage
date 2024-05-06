use std::cmp::max;
use std::collections::HashSet;
use std::ops::{Div, Mul};
use std::sync::Arc;
use std::time::Instant;

use contracts::qv_executor::QVExecutorErrors;
use ethers::prelude::ContractError;
use ethers::providers::{Middleware, StreamExt};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, U256, U64};
use log::{error, info, warn};
use pools_graph::pool_data::pool_data::{PoolData, PoolDataTrait};
use pools_graph::pools_graph::PoolsGraph;
use pools_graph::utils::arbitrage::Arbitrage;
use pools_graph::utils::uniswap_v2;
use rayon::prelude::*;
use utils::logging::setup_logging;
use utils::utils::{FlashbotsProvider, Setup, Utils};

const UNISWAP_V2_FACTORIES: [&str; 5] = [
    // "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
    "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac",
    "0x0388c1e0f210abae597b7de712b9510c6c36c857",
    "0x9DEB29c9a4c7A88a3C0257393b7f3335338D9A9D",
    "0xbdda21dd8da31d5bee0c9bb886c044ebb9b8906a",
    "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362",
];

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

const APP_NAME: &str = env!("CARGO_CRATE_NAME");

const NUMBER_OF_STEPS: u32 = 1000;

const PRIORITY_FEE_PERCENTAGE: u32 = 999;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let utils = Utils::setup().await;
    setup_logging(utils.is_production(), APP_NAME);
    let rpc_client = utils.get_rpc_client();
    let factories = UNISWAP_V2_FACTORIES
        .map(|x| x.parse::<Address>().unwrap())
        .to_vec();
    let graph = Arc::new(PoolsGraph::new());
    uniswap_v2::load_uniswap_v2_pairs(&graph, factories, Arc::clone(&rpc_client)).await?;
    let all_paths = get_all_paths(&graph);
    info!("There are {} paths", all_paths.len());
    // Set of blacklisted addresses because of failures to trade on token
    // let mut blacklist: HashSet<Address> = HashSet::new();
    let ws_client = utils.get_ws_client();
    let mut stream = ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {
        let block_number = block.number.unwrap();
        if block_number != rpc_client.get_block_number().await? {
            continue;
        }
        let next_base_fee = block.next_block_base_fee().unwrap();
        info!("Block number: {block_number} with base fee {next_base_fee}");
        let start = Instant::now();
        update_reserves(
            block_number,
            &all_paths,
            Arc::clone(&graph),
            Arc::clone(&rpc_client),
        )
        .await?;
        info!("It took {:.2?} to refresh all reserves", start.elapsed());

        let mut profitable_trades = all_paths
            .par_iter()
            .map(|x| try_finding_arbitrage(&graph, x))
            .filter(|x| x.is_some())
            .map(|x| x.unwrap())
            .collect::<Vec<_>>();
        profitable_trades.sort();
        while profitable_trades.len() > 0 {
            let top_item = profitable_trades.pop().unwrap();
            let mut cc = top_item.build_transaction(
                &graph,
                WETH.parse()?,
                Arc::clone(&rpc_client),
                utils.get_bundle_executor_address(),
            );

            let gas_estimate_opt = match cc.estimate_gas().await {
                Ok(est) => Some(est),
                Err(e) => {
                    if e.is_revert() {
                        let eth_err: QVExecutorErrors = e.decode_contract_revert().unwrap();
                        warn!("Gas estimate reverted for pairs .... {:#?}", cc.tx);
                    } else {
                        panic!("Got a strange error {e}")
                    }
                    None
                }
            };

            if gas_estimate_opt.is_none() {
                continue;
            }
            let gas_estimate = gas_estimate_opt.unwrap();

            if gas_estimate.mul(next_base_fee) < top_item.get_estimated_profit() {
                info!(
                    "It took {:.2?} to find this trade.. Is this too slow?",
                    start.elapsed()
                );
                match try_submit_trade(
                    &top_item,
                    cc.tx,
                    gas_estimate,
                    next_base_fee,
                    Arc::clone(&rpc_client),
                )
                .await
                {
                    Ok(_) => break,
                    Err(e) => {
                        error!("Failed to submit trade... see error {e}");
                        // need to break because waited until next block to check success
                        break;
                    }
                };
            };
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
        let weth_output_pools = graph
            .get_pool_addresses(input_token.clone(), WETH.parse().unwrap())
            .unwrap()
            .value()
            .iter()
            .map(|x| x.clone())
            .collect::<Vec<Address>>();
        let output_tokens = graph.get_neighbouring_tokens(&input_token.clone()).unwrap();
        for output_token in output_tokens.value().iter() {
            if output_token.0 == WETH.parse::<Address>().unwrap().0 {
                continue;
            }
            let all_pools = graph
                .get_pool_addresses(input_token.clone(), output_token.clone())
                .unwrap();
            for input_pool_address in all_pools.value().iter() {
                for output_pool_address in all_pools.iter() {
                    if output_pool_address.0 != input_pool_address.0 {
                        results.push(Path {
                            input_token: input_token.clone(),
                            input_pool_address: input_pool_address.clone(),
                            output_pool_address: output_pool_address.clone(),
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
                &pool_address,
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
        get_uniswap_v2_pair_reserves(&path.input_pool_address, &graph, zero_for_one);
    let (output_reserve_in, output_reserve_out) =
        get_uniswap_v2_pair_reserves(&path.output_pool_address, &graph, !zero_for_one);
    let max_amount_in = max_amount_in(
        input_reserve_in,
        input_reserve_out,
        output_reserve_in,
        output_reserve_out,
    );
    if max_amount_in > U256::zero() {
        return calculate_profit(&graph, &path, max_amount_in, zero_for_one);
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
            output_address = weth_pool_address.clone();
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

async fn try_submit_trade<M: Middleware + 'static>(
    arb: &Arbitrage,
    tx: TypedTransaction,
    gas_estimate: U256,
    base_fee: U256,
    rpc_client: Arc<M>,
) -> Result<(), Box<dyn std::error::Error>> {
    let remaining_profit = arb.get_estimated_profit() - gas_estimate.mul(base_fee);
    // 50 % of estimated profits
    let max_priority_fee_per_gas = (remaining_profit.div(gas_estimate))
        .mul(U256::from(PRIORITY_FEE_PERCENTAGE))
        .div(U256::from(1000));
    let max_fee = base_fee + max_priority_fee_per_gas;
    info!(
        "Found a trade with estimated profit of {}",
        arb.get_estimated_profit()
    );
    match tx {
        TypedTransaction::Eip1559(inner) => {
            let tx: TypedTransaction = inner
                .gas(gas_estimate)
                .max_priority_fee_per_gas(max_priority_fee_per_gas)
                .max_fee_per_gas(max_fee)
                .chain_id(1)
                .into();
            info!("Sending tx: {:#?}\n", tx);
            let pending_tx = rpc_client.send_transaction(tx, None).await?;
            let receipt = pending_tx
                .await?
                .ok_or_else(|| eyre::format_err!("tx dropped from mempool"))?;
            let tx = rpc_client.get_transaction(receipt.transaction_hash).await?;
            info!("Sent tx: {}\n", serde_json::to_string(&tx)?);
            info!("Tx receipt: {}", serde_json::to_string(&receipt)?);
            return Ok(());
        }
        _other => panic!("Uggh this should be EIP1559"),
    };
}

#[derive(Debug)]
struct Path {
    input_token: Address,
    input_pool_address: Address,
    output_pool_address: Address,
    weth_output_pools: Vec<Address>,
}
