use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use crate::pool_data::pool_data::{PoolData, PoolDataTrait};
use contracts::i_uniswap_v_2_factory::IUniswapV2Factory;
use dashmap::mapref::one::RefMut;
use dashmap::try_result::TryResult;
use ethers::contract::ContractError;
use ethers::prelude::U64;
use ethers::providers::Middleware;
use ethers::types::{Address, U256};
use log::{debug, error, info, warn};

use crate::pool_data::uniswap_v2::UniswapV2;
use crate::pools_graph::PoolsGraph;

pub async fn load_uniswap_v2_pairs<M: Middleware + 'static>(
    pools_graph: &PoolsGraph,
    factory_addresses: Vec<Address>,
    client: Arc<M>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    info!("Loading V2 pools...");
    for factory_address in factory_addresses {
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
                    pools_graph.insert(PoolData::UniswapV2(pool_data));
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

pub async fn refresh_reserves<M: Middleware>(
    pools_graph: &PoolsGraph,
    pair_address: &Address,
    current_block: U64,
    client: Arc<M>,
) -> Result<(), ContractError<M>> {
    match pools_graph.get_mut_pool_data(pair_address) {
        TryResult::Present(mut value) => {
            let value = value.as_any_mut();
            match value.downcast_mut::<UniswapV2>() {
                None => panic!("Refreshing reserves is being called on a non-Uni V2 pool..."),
                Some(val) => val.maybe_refresh_reserves(current_block, client).await?,
            }

            Ok(())
        }
        TryResult::Absent => panic!("{pair_address} not found in pools graph..."),
        TryResult::Locked => {
            warn!("{pair_address} is locked...");
            Ok(())
        }
    }
}
