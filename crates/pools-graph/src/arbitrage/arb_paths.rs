use std::cell::RefCell;
use std::collections::{BinaryHeap, HashMap, HashSet};

use ethers::providers::Middleware;
use ethers::types::{Address, U256, U64};

use crate::arbitrage::arb_paths::ExchangeType::{UniswapV2, UniswapV3};
use crate::arbitrage::arb_tx::ArbTx;
use crate::pool_data::pool_data::{PoolData, PoolDataTrait};
use crate::pools_graph::PoolsGraph;
use crate::utils::EthersCacheDB;

pub struct Arbs<M: Middleware> {
    // All arbitrage path. This should only contain paths that may
    // have arbitrage in a given block, except for the first block where object is built.
    // would be useful to have a zero for one concept on the paths...
    paths: Vec<Vec<ArbPool>>,
    // Heap of valid arbitrage transactions
    // they may have been computed in a previous block,but should always be valid.
    arb_txs: BinaryHeap<ArbTx>,
    // Used to invalidate old ArbTx in arb_txs
    last_valid_tx: HashMap<Vec<ArbPool>, U64>,
    // Cache db used by REVM to compute swaps for UniswapV3, etc.
    cache_db: RefCell<EthersCacheDB<M>>,
}

impl<M: Middleware> Arbs<M> {
    pub fn new(paths: Vec<Vec<ArbPool>>, cache_db: EthersCacheDB<M>) -> Self {
        Self {
            paths,
            arb_txs: BinaryHeap::new(),
            last_valid_tx: HashMap::new(),
            cache_db: RefCell::new(cache_db),
        }
    }

    pub fn compute_all_arbitrages(
        &mut self,
        pools_graph: &PoolsGraph,
        max_amount_in: U256,
        step_size: U256,
        block_number: U64,
    ) {
        for arb_path in &self.paths {
            let arb_tx = self
                .compute_arbitrage(&arb_path, pools_graph, max_amount_in, step_size, block_number)
                .expect("Computed arb shouldn't fail");
            match arb_tx {
                Some(arb) => {
                    // add arb tx to heap
                    self.arb_txs.push(arb);
                    // invalidate other arb tx on that path from previous blocks that
                    // may still be on the heap.
                    self.last_valid_tx.insert(arb_path.to_vec(), block_number);
                }
                None => {
                    // invalidate arb tx that may still be on the heap from previous blocks.
                    self.last_valid_tx.insert(arb_path.to_vec(), block_number);
                }
            }
        }
    }

    fn compute_arbitrage(
        &self,
        arb_path: &Vec<ArbPool>,
        pools_graph: &PoolsGraph,
        max_amount_in: U256,
        step_size: U256,
        block_number: U64,
    ) -> anyhow::Result<Option<ArbTx>> {
        let mut amount_in = U256::zero();
        let mut profitable_arbs = None;
        let mut curr_max_profit = U256::zero();
        while amount_in <= max_amount_in {
            // U256 implements Copy
            let mut prev_amount_in = amount_in;
            // Vec<(amount_in, amount_out)>
            let mut tmp_amounts = Vec::new();
            for arb_pool in arb_path {
                let pool_data = pools_graph
                    .get_pool_data(&arb_pool.pool)
                    .expect("Pool data should not be None");
                let amount_out = pool_data.get_amount_out::<M>(
                    amount_in,
                    arb_pool.token_in,
                    arb_pool.token_out,
                    Some(&mut self.cache_db.borrow_mut()),
                )?;
                tmp_amounts.push((prev_amount_in, amount_out));
                prev_amount_in = amount_out;
            }

            if prev_amount_in > curr_max_profit {
                curr_max_profit = prev_amount_in;
                profitable_arbs = Some(ArbTx::new(
                    arb_path.to_vec(),
                    tmp_amounts.iter().map(|x| x.0).collect(),
                    tmp_amounts.iter().map(|x| x.1).collect(),
                    U256::zero(),
                    None,
                    block_number,
                ))
            }
            amount_in += step_size;
        }
        Ok(profitable_arbs)
    }
}

// Represents a pool in an arbitrage path.
#[derive(Eq, PartialEq, Hash, Debug, Copy, Clone)]
pub struct ArbPool {
    // Pool address.
    pub pool: Address,
    // Input token swap in pool.
    pub token_in: Address,
    // Output token for swap in pool.
    pub token_out: Address,
}

// Use to build an update the `paths` in Arbs.
pub struct PathsBuilder {
    // Length of arbitrage path.
    path_length: u32,
    // Types of exchanges to target.
    targeted_exchanges: HashSet<ExchangeType>,
    // Token to use as output of arbitrage trade. The input token should be the same.
    output_token: Address,
    // Optional input token to the arb path
    input_token: Option<Address>,
    // Optional set of pools to filter on.
    pools: HashSet<Address>,
}

impl PathsBuilder {
    pub fn path_length(self, val: u32) -> Self {
        Self {
            path_length: val,
            ..self
        }
    }
    pub fn targeted_exchanges(self, val: HashSet<ExchangeType>) -> Self {
        Self {
            targeted_exchanges: val,
            ..self
        }
    }
    pub fn output_token(self, val: Address) -> Self {
        Self {
            output_token: val,
            ..self
        }
    }

    pub fn pool(self, val: HashSet<Address>) -> Self {
        Self { pools: val, ..self }
    }

    pub fn input_token(self, val: Address) -> Self {
        Self {
            input_token: Some(val),
            ..self
        }
    }

    pub fn build(self, pools_graph: &PoolsGraph) -> Vec<Vec<ArbPool>> {
        todo!()
        // let mut results = Vec::new();
        // let start_tokens = match self.input_token {
        //     None => pools_graph.get_all_tokens(),
        //     Some(val) => vec![val],
        // };
        // // need to use DFS... but how to make it efficient especially in the
        // // case where there are filtered pools
        //
        // // let start_tokens =
        // results
    }
}

pub enum ExchangeType {
    UniswapV2,
    UniswapV3,
}

impl From<PoolData> for ExchangeType {
    fn from(value: PoolData) -> Self {
        match value {
            PoolData::UniswapV2(_) => UniswapV2,
            PoolData::UniswapV3(_) => UniswapV3,
        }
    }
}
