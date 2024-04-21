use std::env;
use std::sync::Arc;

use ethers::core::k256::ecdsa::SigningKey;
use ethers::middleware::{Middleware, SignerMiddleware};
use ethers::providers::{Http, Provider, Ws};
use ethers::signers::{LocalWallet, Signer, Wallet};
use ethers::types::Address;
use log4rs::{Config, Logger};
use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Root};
use log::LevelFilter;

use pools_graph::pools_graph::PoolsGraph;
use pools_graph::utils::uniswap_v2_loader::load_uniswap_v2_pairs;

const UNISWAP_V2_FACTORIES: [&str; 4] = [
    // "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f",
    // "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac",
    "0x0388c1e0f210abae597b7de712b9510c6c36c857",
    "0x9DEB29c9a4c7A88a3C0257393b7f3335338D9A9D",
    "0xbdda21dd8da31d5bee0c9bb886c044ebb9b8906a",
    "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362",
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (http_client, ws_client) = setup().await;
    let factories: Vec<Address> = UNISWAP_V2_FACTORIES
        .map(|x| x.parse::<Address>().unwrap())
        .to_vec();
    let graph = PoolsGraph::new();
    load_uniswap_v2_pairs(&graph, factories, Arc::clone(&http_client)).await?;
    Ok(())
}

async fn setup() -> (
    Arc<SignerMiddleware<Provider<Http>, Wallet<SigningKey>>>,
    Arc<Provider<Ws>>,
) {
    setup_logging();
    let rpc_url = std::env::var("RPC_URL").unwrap();
    let ws_url = std::env::var("WS_URL").unwrap();
    let private_key = "21212";
    let bundle_executor_address = std::env::var("BUNDLE_EXECUTOR").unwrap();
    let wallet: LocalWallet = private_key.parse().unwrap();
    let wallet = wallet.with_chain_id(1_u32);
    let http_client = if cfg!(debug_assertions) {
        let provider = Provider::<Http>::try_from(rpc_url).unwrap();
        let client = SignerMiddleware::new(provider, wallet);
        Arc::new(client)
    } else {
        //TODO change to flashbots
        let provider = Provider::<Http>::try_from(rpc_url).unwrap();
        let client = SignerMiddleware::new(provider, wallet);
        Arc::new(client)
    };
    let ws_client = Arc::new(Provider::<Ws>::connect(ws_url).await.unwrap());
    (http_client, ws_client)
}

fn setup_logging() {
    // let stdout = ConsoleAppender::builder().build();
    // let package_name = env!("CARGO_PKG_NAME");
    // if cfg!(debug_assertions) {
    //     let config = Config::builder()
    //         .appender(Appender::builder().build("stdout", Box::new(stdout)))
    //         .logger(Logger::builder().build(package_name, LevelFilter::Debug))
    //         .build(Root::builder().appender("stdout").build(LevelFilter::Warn))
    //         .unwrap();
    //     log4rs::init_config(config).expect("Should set up logger in debug mode for app");
    // } else {
    //     let config = Config::builder()
    //         .appender(Appender::builder().build("stdout", Box::new(stdout)))
    //         .logger(Logger::builder().build(package_name, LevelFilter::Info))
    //         .build(Root::builder().appender("stdout").build(LevelFilter::Warn))
    //         .unwrap();
    //     log4rs::init_config(config).expect("Should set up logger in debug mode for app");
    // }
}
