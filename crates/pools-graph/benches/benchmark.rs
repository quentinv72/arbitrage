use std::sync::Arc;

use criterion::{BenchmarkId, black_box, Criterion, criterion_group, criterion_main};
use ethers::prelude::{Http, Provider};
use ethers::types::{Address, U256, U64};
use ethers::utils::{Anvil, WEI_IN_ETHER};
use revm::db::{CacheDB, EthersDB};

use pools_graph::arbitrage::arb_paths::{ArbPool, Arbs};
use pools_graph::pool_data::pool_data::PoolDataTrait;
use pools_graph::pool_data::uniswap_v2::UniswapV2;
use pools_graph::pool_data::uniswap_v3::UniswapV3;
use pools_graph::pools_graph::PoolsGraph;
use utils::placeholder_middleware::PlaceholderMiddleware;

pub fn bench_uniswap_v2_amount_out(c: &mut Criterion) {
    let v2_pool = UniswapV2::new(
        Address::random(),
        Address::random(),
        53295412616945969980934751,
        Address::random(),
        41160837939896193107,
        U64::zero(),
        None,
    );
    let (token_0, token_1) = v2_pool.get_tokens();
    c.bench_function("v2_amount_out", |b| {
        b.iter(|| {
            v2_pool.get_amount_out::<PlaceholderMiddleware>(
                black_box(U256::from(210210)),
                token_0,
                token_1,
                None,
            )
        })
    });
}

// Not the best bench because it depends on network requests
pub fn bench_uniswap_v3_amount_out(c: &mut Criterion) {
    let anvil = Anvil::new()
        .fork("https://eth-sepolia.g.alchemy.com/v2/fEmCuDGqB-tSA4R5HnnVCy1n9Jg4GqJg@6077409")
        .spawn();

    let provider = Arc::new(Provider::<Http>::try_from(anvil.endpoint()).unwrap());

    let pool = UniswapV3::new(
        "0x9799b5EDC1aA7D3FAd350309B08df3F64914E244"
            .parse()
            .unwrap(),
        U256::zero(),
        "0x94a9D9AC8a22534E3FaCa9F4e7F2E2cf85d5E4C8"
            .parse()
            .unwrap(),
        "0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14"
            .parse()
            .unwrap(),
        3,
        None,
    );

    let (token_0, token_1) = pool.get_tokens();

    let ethers_db = EthersDB::new(provider, None).unwrap();

    let mut cache_db = CacheDB::new(ethers_db);

    UniswapV3::load_quoter_bytecode(&mut cache_db);

    c.bench_function("v3_amount_out", |b| {
        b.iter(|| {
            pool.get_amount_out(
                black_box(U256::from(1000)),
                token_0,
                token_1,
                Some(&mut cache_db),
            )
        })
    });
}

fn bench_compute_all_arbitrage(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_all_arbitrage");
    group.sample_size(10);
    let (mut arb, graph) = setup_arb();
    for num_of_steps in [
        U256::from(10),
        U256::from(100),
        U256::from(1_000),
        U256::from(10_000),
    ]
        .iter()
    {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_of_steps),
            num_of_steps,
            |b, &num_steps| {
                b.iter(|| {
                    arb.load_uniswap_v3_quoter_bytecode();
                    arb.compute_all_arbitrages(&graph, WEI_IN_ETHER, num_steps, U64::zero());
                    arb.clear_cache();
                })
            },
        );
    }
}

fn setup_arb() -> (Arbs<Provider<Http>>, PoolsGraph) {
    let provider = Arc::new(
        Provider::<Http>::try_from(
            "http://localhost:8545",
        )
            .unwrap(),
    );

    let ethers_db = EthersDB::new(provider.clone(), None).unwrap();
    let cache_db = CacheDB::new(ethers_db);

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

    let arb_path_2 = vec![
        ArbPool {
            pool: uniswap_v3_pool.pool_address,
            token_in: uniswap_v3_pool.token_1,
            token_out: uniswap_v3_pool.token_0,
        },
        ArbPool {
            pool: uniswap_v3_pool.pool_address,
            token_in: uniswap_v3_pool.token_0,
            token_out: uniswap_v3_pool.token_1,
        },
    ];

    let mut arbs = Arbs::new(vec![arb_path_1, arb_path_2], cache_db);
    (arbs, pools_graph)
}

criterion_group!(
    benches,
    bench_uniswap_v2_amount_out,
    bench_uniswap_v3_amount_out,
    bench_compute_all_arbitrage,
);
criterion_main!(benches);
