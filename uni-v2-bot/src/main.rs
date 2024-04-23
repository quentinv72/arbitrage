use std::collections::HashSet;
use std::sync::Arc;

use ethers::abi::Hash;
use ethers::middleware::Middleware;
use ethers::prelude::ContractError;
use ethers::providers::StreamExt;
use ethers::types::{Address, U64};
use log::{info, warn};
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
    }
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
