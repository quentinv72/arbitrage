use ethers::addressbook::Address;
use ethers::prelude::{Bytes, U256};

use crate::arbitrage::arbs::ArbPool;
use crate::pools_graph::PoolsGraph;

pub mod arb_tx;
pub mod arbs;


pub(crate) trait ExecutorTx {
    // Build new struct T: ExecutorTx.
    fn new(targets: Vec<ArbPool>, amounts_in: Vec<U256>, amounts_out: Vec<U256>) -> Self;

    // Transforms the object into bytes.
    fn build_tx(
        &self,
        pools_graph: &PoolsGraph,
        executor_address: Address,
        output_token: Address,
    ) -> Bytes;

    // Estimated profits.
    fn estimated_profit(&self) -> U256;
}