use ethers::addressbook::Address;
use ethers::prelude::{Bytes, U256};

use crate::arbitrage::arbs::ArbPool;
use crate::pools_graph::PoolsGraph;

pub mod arb_tx_v1;
pub mod arbs;
pub mod executor;
pub mod arb_paths;

pub trait ArbTx {
    fn new(targets: Vec<ArbPool>, amounts_in: Vec<U256>, amounts_out: Vec<U256>) -> Self;
    fn get_bytes(
        &self,
        pools_graph: &PoolsGraph,
        executor_address: Address,
        output_token: Address,
    ) -> Bytes;
    fn estimated_profit(&self) -> U256;
    fn set_amount_to_coinbase(&mut self, amount: U256);
    fn get_amount_to_coinbase(&self) -> U256;
    fn path(&self) -> &[ArbPool];
}
