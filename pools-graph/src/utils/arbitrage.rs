use std::cmp::Ordering;
use std::sync::Arc;

use contracts::qv_executor::QVExecutor;
use ethers::abi::{encode, Token};
use ethers::contract::ContractCall;
use ethers::providers::Middleware;
use ethers::types::{Address, Bytes, U256};

use crate::pools_graph::PoolsGraph;

const BUNDLE_EXECUTOR_ADDRESS: &str = "0x2f5A6dd5bCB5ba085e5f6e2DBF43a0BeA4b6fdfC";

#[derive(Eq, Debug)]
struct Arbitrage {
    targets: Vec<Address>,
    amounts_in: Vec<U256>,
    amounts_out: Vec<U256>,
    zero_for_ones: Vec<bool>,
    amount_to_coinbase: U256,
    estimated_profit: U256,
}

impl PartialEq<Self> for Arbitrage {
    fn eq(&self, other: &Self) -> bool {
        self.estimated_profit == other.estimated_profit
    }
}

impl PartialOrd<Self> for Arbitrage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Arbitrage {
    fn cmp(&self, other: &Self) -> Ordering {
        self.estimated_profit.cmp(&other.estimated_profit)
    }
}

impl Arbitrage {
    fn build_transaction<M: Middleware>(
        &self,
        pools_graph: &PoolsGraph<M>,
        output_token: Address,
        client: Arc<M>,
    ) -> ContractCall<M, ()> {
        let mut next_call = Self::build_data(
            self.amounts_in.last().unwrap().clone(),
            Address::zero(),
            Bytes::from(Vec::new()),
        );
        for i in (1..self.targets.len()).rev() {
            let pool_address = self.targets[i];
            let pool = pools_graph.get_pool_data(&pool_address).unwrap();
            let swap_data = pool.build_swap_calldata(
                self.amounts_in[i],
                self.amounts_out[i],
                self.zero_for_ones[i],
                next_call,
                Arc::clone(&client),
                BUNDLE_EXECUTOR_ADDRESS.parse().unwrap(),
            );
            next_call =
                Self::build_data(self.amounts_in[i - 1].clone(), self.targets[i], swap_data);
        }
        let bundle_executor_contract =
            QVExecutor::new(BUNDLE_EXECUTOR_ADDRESS.parse().unwrap(), client);
        bundle_executor_contract.execute_bundle(
            self.targets[0],
            self.amount_to_coinbase,
            output_token,
            next_call,
        )
    }

    fn build_data(amount_in: U256, target_address: Address, swap_data: Bytes) -> Bytes {
        Bytes::from(encode(&[
            Token::Uint(amount_in),
            Token::Address(target_address),
            Token::Bytes(swap_data.to_vec()),
        ]))
    }
}
