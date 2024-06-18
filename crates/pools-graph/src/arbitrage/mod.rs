use ethers::addressbook::Address;
use ethers::prelude::{Bytes, U256};
use ethers::providers::Middleware;

use crate::arbitrage::arbs::ArbPool;
use crate::pools_graph::PoolsGraph;

pub mod arb_tx_v1;
pub mod arbs;
mod executor;

pub(crate) trait ArbTx {
    fn new(targets: Vec<ArbPool>, amounts_in: Vec<U256>, amounts_out: Vec<U256>) -> Self;
    fn build_tx(
        &self,
        pools_graph: &PoolsGraph,
        executor_address: Address,
        output_token: Address,
    ) -> Bytes;
    fn estimated_profit(&self) -> U256;
}
