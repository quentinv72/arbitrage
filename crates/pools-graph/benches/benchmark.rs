use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ethers::types::{Address, U256, U64};

use pools_graph::pool_data::pool_data::PoolDataTrait;
use pools_graph::pool_data::uniswap_v2::UniswapV2;

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
        b.iter(|| v2_pool.get_amount_out(black_box(U256::from(210210)), black_box(true)))
    });
}

criterion_group!(benches, uniswap_v2_amount_out);
criterion_main!(benches);
