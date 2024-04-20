use std::sync::Arc;
use std::time::Instant;

use contracts::i_uniswap_v_2_factory::IUniswapV2Factory;
use ethers::providers::Middleware;
use ethers::types::{Address, U256};
use log::{debug, error, info};

use crate::pool_data::uniswap_v2::UniswapV2;
use crate::pools_graph::PoolsGraph;

pub async fn load_uniswap_v2_pairs<M: Middleware + 'static>(
    pools_graph: &PoolsGraph,
    factory_addresses: Vec<Address>,
    client: Arc<M>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    info!("Loading V2 pools...");
    for factory in factory_addresses {
        let factory_address: Address = factory.parse()?;
        let client_clone = Arc::clone(&client);
        let factory_contract = IUniswapV2Factory::new(factory_address, client_clone);
        let number_of_pairs = factory_contract.all_pairs_length().call().await?;
        info!(
                "factory address {:#032x} has {number_of_pairs} pairs",
                factory_address
            );
        let mut tasks = Vec::with_capacity(number_of_pairs.as_usize());
        for i in 0..number_of_pairs.as_u32() {
            let client_clone = Arc::clone(&client);
            let pair_address = factory_contract.all_pairs(U256::from(i)).call().await?;
            tasks.push(tokio::spawn(async move {
                UniswapV2::new_from_client(pair_address, client_clone).await
            }));
        }
        for task in tasks {
            match task.await.unwrap() {
                Ok(pool_data) => {
                    pools_graph.insert(pool_data);
                    ()
                }
                Err(e) => error!("Failed fetch reserve {e}"),
            }
        }
    }
    let elapsed = start.elapsed();
    info!("It took {elapsed:.2?} to load all V2 markets");
    Ok(())
}