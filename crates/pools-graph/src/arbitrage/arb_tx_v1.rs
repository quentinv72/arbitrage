use std::cmp::Ordering;

use ethers::abi::AbiEncode;
use ethers::abi::{encode, Token};
use ethers::types::{Address, Bytes, U256};

use contracts::qv_executor::ExecuteBundleCall;

use crate::arbitrage::arbs::ArbPool;
use crate::arbitrage::ArbTx;
use crate::pool_data::pool_data::PoolDataTrait;
use crate::pools_graph::PoolsGraph;

#[derive(Eq, Debug)]
pub struct ArbTxV1 {
    targets: Vec<ArbPool>,
    amounts_in: Vec<U256>,
    amounts_out: Vec<U256>,
    amount_to_coinbase: U256,
    estimated_profit: U256,
}

impl PartialEq<Self> for ArbTxV1 {
    fn eq(&self, other: &Self) -> bool {
        self.estimated_profit == other.estimated_profit
    }
}

impl PartialOrd<Self> for ArbTxV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ArbTxV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.estimated_profit.cmp(&other.estimated_profit)
    }
}

impl ArbTx for ArbTxV1 {
    fn new(targets: Vec<ArbPool>, amounts_in: Vec<U256>, amounts_out: Vec<U256>) -> Self {
        // TODO ad logic to determine amount to send to coinbase
        // An idea is only to add coinbase when estimated profit is obviously going to be
        // substantially larger than potential gas costs.
        let estimated_profit = amounts_out.last().unwrap() - amounts_in.first().unwrap();
        ArbTxV1 {
            targets,
            amounts_in,
            amounts_out,
            estimated_profit,
            amount_to_coinbase: U256::zero(),
        }
    }

    fn get_bytes(
        &self,
        pools_graph: &PoolsGraph,
        executor_address: Address,
        output_token: Address,
    ) -> Bytes {
        let mut next_call = Self::build_data(
            *self.amounts_in.last().unwrap(),
            Address::zero(),
            Bytes::from(Vec::new()),
        );
        for i in (1..self.targets.len()).rev() {
            let pool_address = self.targets[i].pool;
            let pool = pools_graph.get_pool_data(&pool_address).unwrap();
            let swap_data = pool.build_swap_calldata(
                self.amounts_in[i],
                self.amounts_out[i],
                self.targets[i].token_in,
                self.targets[i].token_out,
                next_call,
                executor_address,
            );
            next_call = Self::build_data(self.amounts_in[i - 1], pool_address, swap_data);
        }

        let start_pool_addr = self.targets[0].pool;
        let start_pool = pools_graph.get_pool_data(&start_pool_addr).unwrap();
        next_call = start_pool.build_swap_calldata(
            self.amounts_in[0],
            self.amounts_out[0],
            self.targets[0].token_in,
            self.targets[0].token_out,
            next_call,
            executor_address,
        );

        ExecuteBundleCall {
            target: start_pool_addr,
            amount_to_coinbase: self.amount_to_coinbase,
            output_token,
            data: next_call,
        }
        .encode()
        .into()
    }

    fn estimated_profit(&self) -> U256 {
        self.estimated_profit
    }

    fn set_amount_to_coinbase(&mut self, amount: U256) {
        self.amount_to_coinbase = amount
    }

    fn get_amount_to_coinbase(&self) -> U256 {
        self.amount_to_coinbase
    }

    fn path(&self) -> &[ArbPool] {
        &self.targets
    }
}

impl ArbTxV1 {
    fn build_data(amount_in: U256, target_address: Address, swap_data: Bytes) -> Bytes {
        Bytes::from(encode(&[
            Token::Uint(amount_in),
            Token::Address(target_address),
            Token::Bytes(swap_data.to_vec()),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use ethers::types::{Address, U256, U64};

    use crate::arbitrage::arb_tx_v1::ArbTxV1;
    use crate::arbitrage::arbs::ArbPool;
    use crate::arbitrage::ArbTx;
    use crate::pool_data::uniswap_v2::UniswapV2;
    use crate::pools_graph::PoolsGraph;

    #[test]
    fn build_transaction() {
        let graph = PoolsGraph::default();
        let token_0 = Address::random();
        let token_1 = Address::random();
        let pool_1 = UniswapV2::new(
            "0x615687F0aC866a61dF6550A17812C71d2635bd91"
                .parse()
                .unwrap(),
            token_0,
            10,
            token_1,
            10,
            U64::zero(),
            None,
        )
        .into();
        let pool_2 = UniswapV2::new(
            "0xe6CE0226859f99C095c5b405BF187dC3c55Ab4D8"
                .parse()
                .unwrap(),
            token_0,
            10,
            token_1,
            10,
            U64::zero(),
            None,
        )
        .into();
        graph.insert(pool_1);
        graph.insert(pool_2);
        let input_arb_pool = ArbPool {
            pool: "0x615687F0aC866a61dF6550A17812C71d2635bd91"
                .parse()
                .unwrap(),
            token_in: token_1,
            token_out: token_0,
        };

        let output_arb_pool = ArbPool {
            pool: "0xe6CE0226859f99C095c5b405BF187dC3c55Ab4D8"
                .parse()
                .unwrap(),
            token_in: token_0,
            token_out: token_1,
        };
        let arbitrage = ArbTxV1 {
            targets: vec![input_arb_pool, output_arb_pool],
            amounts_in: vec![
                U256::from_dec_str("6314425151630").unwrap(),
                U256::from_dec_str("15842350854553147119").unwrap(),
            ],
            amounts_out: vec![
                U256::from_dec_str("15842350854553147119").unwrap(),
                U256::from_dec_str("441375928197785").unwrap(),
            ],
            amount_to_coinbase: U256::zero(),
            estimated_profit: U256::zero(),
        };
        let bundle_executor_address = "0x2f5A6dd5bCB5ba085e5f6e2DBF43a0BeA4b6fdfC"
            .parse()
            .unwrap();
        let bytes = arbitrage.get_bytes(
            &graph,
            bundle_executor_address,
            "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
                .parse()
                .unwrap(),
        );
        let tx = format!("{}", bytes);
        assert_eq!(tx, "0x32566586000000000000000000000000615687f0ac866a61df6550a17812c71d2635bd910000000000000000000000000000000000000000000000000000000000000000000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc200000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000264022c0d9f000000000000000000000000000000000000000000000000dbdb562a74bf16ef00000000000000000000000000000000000000000000000000000000000000000000000000000000000000002f5a6dd5bcb5ba085e5f6e2dbf43a0bea4b6fdfc000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000000000001c0000000000000000000000000000000000000000000000000000005be3111708e000000000000000000000000e6ce0226859f99c095c5b405bf187dc3c55ab4d800000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000124022c0d9f00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001916dd769a2990000000000000000000000002f5a6dd5bcb5ba085e5f6e2dbf43a0bea4b6fdfc00000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000080000000000000000000000000000000000000000000000000dbdb562a74bf16ef0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000")
    }
}
