use std::sync::Arc;
use std::time::Instant;

use dashmap::try_result::TryResult;
use ethers::contract::ContractError;
use ethers::prelude::U64;
use ethers::providers::Middleware;
use ethers::types::{Address, U256};
use log::{error, info, warn};
use once_cell::sync::Lazy;

use contracts::i_uniswap_v_2_factory::IUniswapV2Factory;

use crate::pool_data::factory::FactoryV2;
use crate::pool_data::pool_data::PoolData;
use crate::pool_data::uniswap_v2::UniswapV2;
use crate::pools_graph::PoolsGraph;

pub static UNISWAP_V2_FACTORY: Lazy<FactoryV2> = Lazy::new(|| FactoryV2 {
    address: "0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f"
        .parse()
        .unwrap(),
    swap_fee: U256::from(3),
});

pub static SUSHISWAP_FACTORY: Lazy<FactoryV2> = Lazy::new(|| FactoryV2 {
    address: "0xC0AEe478e3658e2610c5F7A4A2E1777cE9e4f2Ac"
        .parse()
        .unwrap(),
    swap_fee: U256::from(3),
});

pub static LUA_SWAP_FACTORY: Lazy<FactoryV2> = Lazy::new(|| FactoryV2 {
    address: "0x0388c1e0f210abae597b7de712b9510c6c36c857"
        .parse()
        .unwrap(),
    swap_fee: U256::from(4),
});

pub static CRO_DEFI_FACTORY: Lazy<FactoryV2> = Lazy::new(|| FactoryV2 {
    address: "0x9DEB29c9a4c7A88a3C0257393b7f3335338D9A9D"
        .parse()
        .unwrap(),
    swap_fee: U256::from(3),
});

pub static ZEUS_FACTORY: Lazy<FactoryV2> = Lazy::new(|| FactoryV2 {
    address: "0xbdda21dd8da31d5bee0c9bb886c044ebb9b8906a"
        .parse()
        .unwrap(),
    swap_fee: U256::from(3),
});

pub static PANCAKE_SWAP_FACTORY: Lazy<FactoryV2> = Lazy::new(|| FactoryV2 {
    address: "0x1097053Fd2ea711dad45caCcc45EfF7548fCB362"
        .parse()
        .unwrap(),
    swap_fee: U256::from(3),
});

pub async fn load_uniswap_v2_pairs<M: Middleware + 'static>(
    pools_graph: &PoolsGraph,
    factory_addresses: Vec<FactoryV2>,
    client: Arc<M>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    info!("Loading V2 pools...");
    for factory in factory_addresses {
        let client_clone = Arc::clone(&client);
        let factory_contract = IUniswapV2Factory::new(factory.address, client_clone);
        let number_of_pairs = factory_contract.all_pairs_length().call().await?;
        info!(
            "factory address {:#032x} has {number_of_pairs} pairs",
            factory.address
        );
        let mut tasks = Vec::with_capacity(number_of_pairs.as_usize());
        for i in 0..number_of_pairs.as_u32() {
            let client_clone = Arc::clone(&client);
            let pair_address = factory_contract.all_pairs(U256::from(i)).call().await?;
            tasks.push(tokio::spawn(async move {
                UniswapV2::new_from_client(pair_address, factory, client_clone).await
            }));
        }
        for task in tasks {
            match task.await.unwrap() {
                Ok(pool_data) => {
                    pools_graph.insert(PoolData::UniswapV2(pool_data));
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
            match value.value_mut() {
                PoolData::UniswapV2(inner) => {
                    inner
                        .maybe_refresh_reserves(Some(current_block), client)
                        .await?
                }
                PoolData::UniswapV3(_) => {
                    panic!("Refreshing reserves is being called on a non-Uni V2 pool...")
                }
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
