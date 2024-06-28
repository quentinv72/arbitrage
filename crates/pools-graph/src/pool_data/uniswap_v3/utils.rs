use std::sync::Arc;
use std::time::Instant;

use ethers::middleware::Middleware;
use log::{error, info};
use once_cell::sync::Lazy;
use reqwest_graphql::Client;
use serde::{Deserialize, Serialize};

use crate::pool_data::factory::FactoryV3;
use crate::pool_data::pool_data::PoolData;
use crate::pool_data::uniswap_v3::UniswapV3;
use crate::pools_graph::PoolsGraph;

pub static UNISWAP_V3_FACTORY: Lazy<FactoryV3> = Lazy::new(|| {
    FactoryV3 {
    address: "0x1F98431c8aD98523631AE4a59f267346ea31F984"
        .parse()
        .unwrap(),
    subgraph_url: "https://gateway-arbitrum.network.thegraph.com/api/130059492893eb84c05bc86c467f148d/subgraphs/id/5zvR82QoaXYFyDEKLZ9t6v9adgnptxYpKpSbxtgVENFV",
    quoter_address:"0x61fFE014bA17989E743c5F6cB21bF9697530B21e".parse().unwrap(),
}
});

pub static PANCAKE_SWAP_V3: Lazy<FactoryV3> = Lazy::new(|| {
    FactoryV3 {
    address: "0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865"
        .parse()
        .unwrap(),
    subgraph_url: "https://gateway-arbitrum.network.thegraph.com/api/130059492893eb84c05bc86c467f148d/subgraphs/id/CJYGNhb7RvnhfBDjqpRnD3oxgyhibzc7fkAMa38YV3oS",
    quoter_address: "0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997".parse().unwrap(),
}
});

pub static SUSHI_SWAP_V3: Lazy<FactoryV3> = Lazy::new(|| {
    FactoryV3 {
    address: "0xbACEB8eC6b9355Dfc0269C18bac9d6E2Bdc29C4F"
        .parse()
        .unwrap(),
    subgraph_url: "https://gateway-arbitrum.network.thegraph.com/api/130059492893eb84c05bc86c467f148d/subgraphs/id/7okunX6MGm2pdFK7WJSwm9o82okpBLEzfGrqHDDMWYvq",
    quoter_address: "0x64e8802FE490fa7cc61d3463958199161Bb608A7".parse().unwrap(),
}
});

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

pub trait LoadQuoterV3<M: Middleware> {
    fn load_uniswap_v3_quoter(&mut self);
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
