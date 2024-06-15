use std::sync::Arc;

use criterion::{BatchSize, black_box, Criterion, criterion_group, criterion_main};
use ethers::prelude::{Http, Provider};
use ethers::types::{Address, U256, U64};
use revm::db::{CacheDB, EthersDB};

use pools_graph::pool_data::pool_data::PoolDataTrait;
use pools_graph::pool_data::uniswap_v2::UniswapV2;
use pools_graph::pool_data::uniswap_v3::UniswapV3;
use pools_graph::utils::EthersCacheDB;
use utils::placeholder_middleware::PlaceholderMiddleware;

pub fn bench_uniswap_v2_amount_out(c: &mut Criterion) {
    let mut group = c.benchmark_group("bench_uniswap_v2_amount_out");
    group.sample_size(500);
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
    group.bench_function("v2_amount_out", |b| {
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

// This bench test only tests get_amount_out when nothing has been cached in Ethers Db
// To get an idea of what it looks like with cache, look at the compute_arbitrage results.
pub fn bench_uniswap_v3_amount_out(c: &mut Criterion) {
    let mut group = c.benchmark_group("bench_uniswap_v3_amount_out");
    group.sample_size(100);
    let pool = UniswapV3 {
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

    let (token_0, token_1) = pool.get_tokens();


    group.bench_function("v3_amount_out", |b| {
        b.iter_batched(setup_cache_db, |mut cache_db| {
            pool.get_amount_out(
                black_box(U256::from(1000)),
                token_0,
                token_1,
                Some(&mut cache_db),
            )
        }, BatchSize::SmallInput)
    });
}

fn setup_cache_db() -> EthersCacheDB<Provider<Http>> {
    let provider = Arc::new(
        Provider::<Http>::try_from(
            "http://localhost:8545",
        )
            .unwrap(),
    );
    let ethers_db = EthersDB::new(provider, None).unwrap();
    let mut cache_db = CacheDB::new(ethers_db);
    UniswapV3::load_quoter_bytecode(&mut cache_db);
    cache_db
}

criterion_group!(
    benches,
    bench_uniswap_v2_amount_out,
    bench_uniswap_v3_amount_out,
);
criterion_main!(benches);