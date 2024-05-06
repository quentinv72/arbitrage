use std::cmp::max;
use std::collections::{BinaryHeap, HashSet};
use std::error::Error;
use std::ops::{Div, Mul};
use std::sync::Arc;

use contracts::qv_executor::QVExecutorErrors;
use ethers::abi::Hash;
use ethers::contract::{ContractCall, EthError};
use ethers::core::k256::elliptic_curve::consts::U2;
use ethers::middleware::Middleware;
use ethers::prelude::{ContractError, SignerMiddleware};
use ethers::prelude::transaction::eip2718::TypedTransaction;
use ethers::providers::StreamExt;
use ethers::signers::Signer;
use ethers::types::{Address, U256, U64};
use ethers::utils::Units::Gwei;
use ethers_flashbots::{BroadcasterMiddleware, BundleRequest, PendingBundleError};
use log::{debug, error, info, warn};
use pools_graph::pool_data::pool_data::{PoolData, PoolDataTrait};
use pools_graph::pool_data::uniswap_v2::UniswapV2;
use pools_graph::pools_graph::PoolsGraph;
use pools_graph::utils::arbitrage::Arbitrage;
use pools_graph::utils::uniswap_v2;
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;
use tokio::task::JoinSet;
use tokio::time::Instant;
use utils::logging::setup_logging;
use utils::utils::{FlashbotsProvider, Setup, Utils};

const UNISWAP_V2_FACTORIES: [&str; 5] = [
    "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
    "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac",
    "0x0388c1e0f210abae597b7de712b9510c6c36c857",
    // TODO https://github.com/quentinv72/arbitrage/issues/1
    // "0x9DEB29c9a4c7A88a3C0257393b7f3335338D9A9D",
    "0xbdda21dd8da31d5bee0c9bb886c044ebb9b8906a",
    "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362",
];

const WETH: &str = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";

// There is probably some env variable that I can use here...
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
    let paths = get_paths_2(&graph, WETH.parse()?);
    info!("Found {} paths", paths.len());
    // Set of blacklisted addresses because of failures to trade on token
    let mut blacklist: HashSet<Address> = HashSet::new();
    let ws_client = utils.get_ws_client();
    let mut stream = ws_client.subscribe_blocks().await?;
    while let Some(block) = stream.next().await {
        let start = Instant::now();
        let block_number = block.number.unwrap();
        if block_number != rpc_client.get_block_number().await? {
            continue;
        }
        let next_base_fee = block.next_block_base_fee().unwrap();
        info!("Block number: {block_number} with base fee {next_base_fee}");
        update_reserves(block_number, &paths, graph.clone(), Arc::clone(&rpc_client)).await?;
        let mut profitable_trades = paths
            .par_iter()
            .filter(|x| !blacklist.contains(&x.0) && !blacklist.contains(&x.1))
            .map(|x| try_finding_arbitrage(&graph, x.0, x.1))
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
                Ok(est) => {
                    top_item.get_pair_addresses().iter().for_each(|x| {
                        if blacklist.contains(x) {
                            info!("Removing {x} from blacklist");
                            blacklist.remove(x);
                        }
                    });
                    Some(est)
                }
                Err(e) => {
                    if e.is_revert() {
                        let eth_err: QVExecutorErrors = e.decode_contract_revert().unwrap();
                        top_item.get_pair_addresses().iter().for_each(|x| {
                            blacklist.insert(x.clone());
                            ()
                        });
                        warn!(
                            "Bad arbitrage... Gas estimate reverted. Blacklisting pairs {:#?} \n\
                            Revert data --- {eth_err}",
                            top_item.get_pair_addresses()
                        );
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
                let client_clone = Arc::clone(&rpc_client);
                tokio::spawn(async move {
                    match try_submit_trade(
                        &top_item,
                        cc.tx,
                        gas_estimate,
                        next_base_fee,
                        client_clone,
                    )
                    .await
                    {
                        Ok(_) => (),
                        Err(e) => {
                            error!("Failed to submit trade... see error {e}");
                            // need to break because waited until next block to check success
                        }
                    };
                });
            };
        }
    }

    Ok(())
}

fn try_finding_arbitrage(
    graph: &PoolsGraph,
    input_address: Address,
    output_address: Address,
) -> Option<Arbitrage> {
    let (input_token_0, _) = graph.get_pool_data(&input_address).unwrap().get_tokens();
    let zero_for_one = input_token_0 == WETH.parse().unwrap();
    let (input_reserve_in, input_reserve_out) =
        get_uniswap_v2_pair_reserves(&input_address, &graph, zero_for_one);
    let (output_reserve_in, output_reserve_out) =
        get_uniswap_v2_pair_reserves(&output_address, &graph, !zero_for_one);
    let max_amount_in = max_amount_in(
        input_reserve_in,
        input_reserve_out,
        output_reserve_in,
        output_reserve_out,
    );
    if max_amount_in > U256::zero() {
        let (amount_in_first, amount_out_first, amount_out_second, profit) = calculate_profit(
            &graph,
            &input_address,
            &output_address,
            max_amount_in,
            zero_for_one,
        );
        if profit > U256::zero() {
            return Some(Arbitrage::new(
                vec![input_address, output_address],
                vec![amount_in_first, amount_out_first],
                vec![amount_out_first, amount_out_second],
                vec![zero_for_one, !zero_for_one],
                U256::zero(),
            ));
        }
    }
    None
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

fn calculate_profit(
    pools_graph: &PoolsGraph,
    input_pair_address: &Address,
    output_pair_address: &Address,
    max_amount_in: U256,
    zero_for_one: bool,
) -> (U256, U256, U256, U256) {
    let mut i = U256::one();
    let step_size = max(U256::one(), max_amount_in.div(U256::from(NUMBER_OF_STEPS)));
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
