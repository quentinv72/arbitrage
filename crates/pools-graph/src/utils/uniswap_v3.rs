use std::sync::Arc;
use std::time::Instant;

use ethers::middleware::Middleware;
use log::{error, info};
use reqwest_graphql::Client;
use serde::{Deserialize, Serialize};

use crate::pool_data::factory::FactoryV3;
use crate::pool_data::pool_data::PoolData;
use crate::pool_data::uniswap_v3::UniswapV3;
use crate::pools_graph::PoolsGraph;

pub async fn load_uniswap_v3_pools<M: Middleware + 'static>(
    pools_graph: &PoolsGraph,
    factories: Vec<FactoryV3>,
    client: Arc<M>,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    for factory in factories {
        let pool_count = get_total_v3_pools(factory.subgraph_url).await.unwrap();
        info!("Factory {} has {pool_count} pools", factory.address);
        let mut id_gt = String::from("");
        loop {
            let vars = Vars {
                first: 1000,
                id_gt: id_gt.clone(),
            };
            let pools = get_pools(factory.subgraph_url, vars).await?;
            if pools.is_empty() {
                break;
            }
            let mut tasks = Vec::with_capacity(pools.len());
            id_gt.clone_from(&pools.last().unwrap().id);
            for pool in pools {
                let client_clone = Arc::clone(&client);
                tasks.push(tokio::spawn(async move {
                    UniswapV3::new_from_client(
                        pool.id.parse().unwrap(),
                        pool.token0.id.parse().unwrap(),
                        pool.token1.id.parse().unwrap(),
                        pool.fee_tier.parse().unwrap(),
                        factory,
                        client_clone,
                    )
                    .await
                }))
            }
            for task in tasks {
                match task.await.unwrap() {
                    Ok(pool_data) => pools_graph.insert(PoolData::UniswapV3(pool_data)),
                    Err(e) => error!("Failure to load pool data into map {e}"),
                }
            }
        }
    }
    let elapsed = start.elapsed();
    info!("It took {elapsed:.2?} to load all V3 markets");
    Ok(())
}

async fn get_total_v3_pools(url: &str) -> Result<u32, Box<dyn std::error::Error>> {
    let client = Client::new(url);
    let query = r#"
            {
                factories {
                        poolCount
                    }
               }
        "#;
    match client.query::<FactoriesData>(query).await {
        Ok(factory_data) => {
            let pool_count_str = &factory_data.factories.first().unwrap().pool_count;
            let pool_count = pool_count_str.parse::<u32>()?;
            Ok(pool_count)
        }
        Err(e) => {
            error!("Failure to get pool count for {url}. Got the following error {e}");
            Ok(0)
        }
    }
}

async fn get_pools(url: &str, vars: Vars) -> Result<Vec<Pool>, Box<dyn std::error::Error>> {
    let client = Client::new(url);
    let query = r#"
            query Pools($first: Int, $id_gt: ID) {
                pools(first: $first, where: {id_gt: $id_gt}) {
                    id
                    token0 {
                        id
                    }
                    token1 {
                        id
                    }
                    feeTier
                }
            }"#;
    match client.query_with_vars::<Pools, Vars>(query, vars).await {
        Ok(pools_data) => Ok(pools_data.pools),
        Err(e) => {
            error!("{e}");
            Ok(Vec::new())
        }
    }
}

#[derive(Serialize)]
struct Vars {
    first: u32,
    id_gt: String,
}

#[derive(Deserialize)]
struct Pools {
    pools: Vec<Pool>,
}

#[derive(Deserialize)]
struct Pool {
    token0: Token,
    token1: Token,
    #[serde(rename = "feeTier")]
    fee_tier: String,
    id: String,
}

#[derive(Deserialize)]
struct Token {
    id: String,
}

#[derive(Deserialize)]
struct FactoriesData {
    factories: Vec<PoolCount>,
}

#[derive(Deserialize)]
struct PoolCount {
    #[serde(rename = "poolCount")]
    pool_count: String,
}
