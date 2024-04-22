use std::sync::Arc;

use ethers::middleware::Middleware;
use ethers::signers::Signer;
use ethers::types::Address;
use pools_graph::pools_graph::PoolsGraph;
use pools_graph::utils::uniswap_v2_loader::load_uniswap_v2_pairs;
use utils::env::Env;
use utils::logging::setup_logging;

const UNISWAP_V2_FACTORIES: [&str; 5] = [
    // "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
    "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac",
    "0x0388c1e0f210abae597b7de712b9510c6c36c857",
    "0x9DEB29c9a4c7A88a3C0257393b7f3335338D9A9D",
    "0xbdda21dd8da31d5bee0c9bb886c044ebb9b8906a",
    "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362",
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env = Env::get_env().await;
    setup_logging(&env, "uni_v2_bot");
    // figure out a way to pick clinet based on environment
    let client = env.get_staging_rpc_client().unwrap();
    let factories = UNISWAP_V2_FACTORIES
        .map(|x| x.parse::<Address>().unwrap())
        .to_vec();
    let graph = PoolsGraph::new();
    load_uniswap_v2_pairs(&graph, factories, Arc::clone(&client)).await?;
    Ok(())
}
