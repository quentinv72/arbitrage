use std::sync::Arc;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use ethers::addressbook::Address;
use ethers::prelude::{Http, Provider, U256, U64};
use ethers::utils::WEI_IN_ETHER;
use revm::db::{CacheDB, EthersDB};

use pools_graph::arbitrage::arb_paths::ArbPaths;
use pools_graph::arbitrage::arb_tx_v1::ArbTxV1;
use pools_graph::arbitrage::arbs::{ArbPool, Arbs};
use pools_graph::arbitrage::executor::Executor;
use pools_graph::pool_data::uniswap_v3::UniswapV3;
use pools_graph::pool_data::uniswap_v3::utils::LoadQuoterV3;
use pools_graph::pools_graph::PoolsGraph;

fn bench_compute_all_arbitrage_2_v3_pools(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_all_arbitrage_2_v3_pools");
    group.sample_size(100);
    for num_of_steps in [U256::from(100), U256::from(500), U256::from(1_000)].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_of_steps),
            num_of_steps,
            |b, &num_steps| {
                b.iter_batched(
                    setup_arb,
                    |(mut arb, graph)| {
                        arb.load_uniswap_v3_quoter();
                        arb.compute_all_arbitrages(
                            &["0xb457fcd59cbe5cb116d1f649fa0f921b42557aef"
                                .parse()
                                .unwrap()],
                            &graph,
                            WEI_IN_ETHER,
                            num_steps,
                        );
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
}

#[allow(clippy::type_complexity)]
fn setup_arb() -> (
    Arbs<Provider<Http>, ArbTxV1, Executor<Provider<Http>>>,
    PoolsGraph,
) {
    let provider = Arc::new(Provider::<Http>::try_from("http://localhost:8545").unwrap());

    let ethers_db = EthersDB::new(provider.clone(), None).unwrap();
    let cache_db = CacheDB::new(ethers_db);

    let default_executor = Executor {
        client: provider,
        executor_address: Address::random(),
        sender: Address::random(),
        output_token: Address::random(),
        chain_id: U64::zero(),
        tip_percentage: Default::default(),
        coinbase_threshold: U256::one(),
    };

    let uniswap_v3_pool = UniswapV3 {
        pool_address: "0xb457fcd59cbe5cb116d1f649fa0f921b42557aef"
            .parse()
            .unwrap(),
        token_0: "0x1e971b5b21367888239f00Da16F0A6b0efFeCb03"
            .parse()
            .unwrap(),
        token_1: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            .parse()
            .unwrap(),
        ..Default::default()
    };

    let pools_graph = PoolsGraph::default();
    pools_graph.insert(uniswap_v3_pool.clone().into());

    let arb_path_1 = vec![
        ArbPool {
            pool: uniswap_v3_pool.pool_address,
            token_in: uniswap_v3_pool.token_0,
            token_out: uniswap_v3_pool.token_1,
        },
        ArbPool {
            pool: uniswap_v3_pool.pool_address,
            token_in: uniswap_v3_pool.token_1,
            token_out: uniswap_v3_pool.token_0,
        },
    ];

    let mut arb_paths = ArbPaths::default();
    arb_paths.insert_path(arb_path_1).unwrap();

    let arbs = Arbs::new(arb_paths, cache_db, default_executor, U256::zero());
    (arbs, pools_graph)
}

criterion_group!(benches, bench_compute_all_arbitrage_2_v3_pools);
criterion_main!(benches);
