use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ethers::prelude::{Http, Provider};
use ethers::types::{Address, U256, U64};
use ethers::utils::Anvil;
use revm::db::{CacheDB, EthersDB};

use pools_graph::pool_data::pool_data::PoolDataTrait;
use pools_graph::pool_data::uniswap_v2::UniswapV2;
use pools_graph::pool_data::uniswap_v3::UniswapV3;
use utils::placeholder_middleware::PlaceholderMiddleware;

pub fn uniswap_v2_amount_out(c: &mut Criterion) {
    let v2_pool = UniswapV2::new(
        Address::random(),
        Address::random(),
        53295412616945969980934751,
        Address::random(),
        41160837939896193107,
        U64::zero(),
        None,
    );
    c.bench_function("v2_amount_out", |b| {
        b.iter(|| {
            v2_pool.get_amount_out::<PlaceholderMiddleware>(
                black_box(U256::from(210210)),
                black_box(true),
                None,
            )
        })
    });
}

pub fn uniswap_v3_amount_out(c: &mut Criterion) {
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

    let ethers_db = EthersDB::new(provider, None).unwrap();

    let mut cache_db = CacheDB::new(ethers_db);

    UniswapV3::load_quoter_bytecode(&mut cache_db);

    c.bench_function("v3_amount_out", |b| {
        b.iter(|| {
            pool.get_amount_out(
                black_box(U256::from(1000)),
                black_box(true),
                Some(&mut cache_db),
            )
        })
    });
}

criterion_group!(benches, uniswap_v2_amount_out, uniswap_v3_amount_out);
criterion_main!(benches);
