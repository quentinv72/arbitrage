use std::cell::RefCell;
use std::collections::{BinaryHeap, HashMap};
use std::ops::Div;
use std::str::FromStr;

use ethers::providers::Middleware;
use ethers::types::{Address, U256, U64};
use revm::precompile::B256;
use revm::primitives::{alloy_primitives, ruint, AccountInfo, Bytecode, KECCAK_EMPTY};

use crate::arbitrage::arb_tx::ArbTx;
use crate::pool_data::pool_data::PoolDataTrait;
use crate::pool_data::uniswap_v3::{QUOTER_BYTECODE, QUOTER_MOCK_ADDRESS};
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

    // This method should be called at the end of each block to ensure that
    // the cached values from the current block aren't used for the next block.
    // Based on the CacheDb::new method implementation.
    pub fn clear_cache(&mut self) {
        self.cache_db.borrow_mut().contracts.clear();
        self.cache_db.borrow_mut().accounts.clear();
        self.cache_db.borrow_mut().block_hashes.clear();
        self.cache_db
            .borrow_mut()
            .contracts
            .insert(KECCAK_EMPTY, Bytecode::default());
        self.cache_db
            .borrow_mut()
            .contracts
            .insert(B256::ZERO, Bytecode::default());
    }

    // Should be called at the start of each block to ensure that the
    pub fn load_uniswap_v3_quoter_bytecode(&mut self) {
        let bytes = alloy_primitives::Bytes::from_str(QUOTER_BYTECODE).unwrap();
        let bytecode = Bytecode::new_raw(bytes);
        self.cache_db.borrow_mut().insert_account_info(
            QUOTER_MOCK_ADDRESS,
            AccountInfo {
                balance: ruint::aliases::U256::from(0),
                nonce: 1,
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode),
            },
        )
    }

    pub fn compute_all_arbitrages(
        &mut self,
        pools_graph: &PoolsGraph,
        max_amount_in: U256,
        num_steps: U256,
        block_number: U64,
    ) {
        // this can probably be parallelized by making clones of cache db for each arb path..
        // This might be quicker but not sure.
        // Needs to be benchmarked.
        for arb_path in &self.paths {
            let arb_tx = self
                .compute_arbitrage(
                    arb_path,
                    pools_graph,
                    max_amount_in,
                    num_steps,
                    block_number,
                )
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
        num_steps: U256,
        block_number: U64,
    ) -> anyhow::Result<Option<ArbTx>> {
        let mut amount_in = U256::zero();
        let mut profitable_arbs = None;
        let mut curr_max_profit = U256::zero();
        let step_size = max_amount_in.div(num_steps);
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

            if prev_amount_in > amount_in && prev_amount_in - amount_in > curr_max_profit {
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

#[cfg(test)]
mod arbs_tests {
    use std::sync::Arc;

    use ethers::prelude::{Http, Provider};
    use ethers::types::{U256, U64};
    use ethers::utils::Anvil;
    use revm::db::{CacheDB, EthersDB};

    use crate::arbitrage::arb_paths::{ArbPool, Arbs};
    use crate::pool_data::uniswap_v2::UniswapV2;
    use crate::pool_data::uniswap_v3::UniswapV3;
    use crate::pools_graph::PoolsGraph;

    #[tokio::test(flavor = "multi_thread")]
    async fn compute_all_arbitrage() {
        // https://dashboard.tenderly.co/tx/mainnet/0x5b7ab63e7b23e37eca4a3723e7df22897613da681e6d0d47f689177218b0ecb9?trace=0
        // Inspiration for the test
        let block_number = U64::from(20077116);
        let anvil = Anvil::new()
            .fork("https://eth-mainnet.g.alchemy.com/v2/Xc1e5k88oOw8atNHzpPXbSw3pKrQ2a4-@20077116")
            .spawn();

        let provider = Arc::new(Provider::<Http>::try_from(anvil.endpoint()).unwrap());

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

        let mut uniswap_v2_pair = UniswapV2 {
            pair_address: "0x3574948e6ba1d48a57f4ade944bc0e4eb20f7d5e"
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

        // Update reserves for the Uniswap V2 pair
        uniswap_v2_pair
            .maybe_refresh_reserves(Some(block_number), provider.clone())
            .await
            .unwrap();

        let pools_graph = PoolsGraph::default();
        pools_graph.insert(uniswap_v2_pair.clone().into());
        pools_graph.insert(uniswap_v3_pool.clone().into());

        let arb_path = vec![
            ArbPool {
                pool: uniswap_v3_pool.pool_address,
                token_in: uniswap_v3_pool.token_0,
                token_out: uniswap_v3_pool.token_1,
            },
            ArbPool {
                pool: uniswap_v2_pair.pair_address,
                token_in: uniswap_v2_pair.token_1,
                token_out: uniswap_v2_pair.token_0,
            },
        ];

        let mut arbs = Arbs::new(vec![arb_path.clone()], cache_db);

        arbs.load_uniswap_v3_quoter_bytecode();

        arbs.compute_all_arbitrages(
            &pools_graph,
            U256::from_dec_str("505000000000000000000000").unwrap(),
            U256::from(100),
            block_number,
        );
        assert_eq!(arbs.arb_txs.len(), 1);
        assert_eq!(
            arbs.arb_txs.peek().unwrap().estimated_profit(),
            U256::from_dec_str("1116824102838017552066455359").unwrap()
        );
        assert_eq!(arbs.last_valid_tx.get(&arb_path).unwrap(), &block_number);
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

// ####### UNTESTED and NOT USING FOR NOW #######
// Reason: Too complex, seems simpler to write custom functions for each bot....
// Use to build an update the `paths` in Arbs.
// pub struct PathsBuilder {
//     // Length of arbitrage path.
//     path_length: u8,
//     // Token to use as output of arbitrage trade. The input token should be the same.
//     output_token: Address,
//     // Optional input token to the arb path
//     input_token: Option<Address>,
//     // Optional set of pools to filter on.
//     pools: HashSet<Address>,
// }
//
// impl PathsBuilder {
//     pub fn path_length(self, val: u8) -> Self {
//         Self {
//             path_length: val,
//             ..self
//         }
//     }
//
//     pub fn output_token(self, val: Address) -> Self {
//         Self {
//             output_token: val,
//             ..self
//         }
//     }
//
//     pub fn pool(self, val: HashSet<Address>) -> Self {
//         Self { pools: val, ..self }
//     }
//
//     pub fn input_token(self, val: Address) -> Self {
//         Self {
//             input_token: Some(val),
//             ..self
//         }
//     }
//
//     pub fn build(self, pools_graph: &PoolsGraph) -> Vec<Vec<ArbPool>> {
//         // Requirements
//         // 1. Every pool must be unique in a path
//         // 2. Paths should have length == self.path_length
//         // 3. start of every pool should be either self.input_token or any token in the graph
//         // 4. Every pool must have output token of self.output_token
//         // 5. Targeted exchanges should only be from self.targeted_exchanges
//
//         let start_tokens = match self.input_token {
//             None => pools_graph.get_all_tokens(),
//             Some(val) => vec![val],
//         };
//
//         let token_paths = self.token_paths(start_tokens, pools_graph);
//         token_paths
//             .par_iter()
//             .map(|path| {
//                 // below represents a list of every possible pool for each pair in the
//                 // path.
//                 path.windows(2)
//                     .map(|tokens| {
//                         let input_token = tokens[0];
//                         let output_token = tokens[1];
//                         pools_graph
//                             .get_pool_addresses(input_token, output_token)
//                             .unwrap()
//                             .iter()
//                             .map(|_pool| ArbPool {
//                                 pool: *_pool,
//                                 token_in: input_token,
//                                 token_out: output_token,
//                             })
//                             .collect()
//                     })
//                     .collect::<Vec<Vec<ArbPool>>>()
//             })
//             // Take cartesian product of pools associated with each pair in a token path and filter them.
//             .flat_map(|pool_paths| {
//                 pool_paths
//                     .iter()
//                     .multi_cartesian_product()
//                     .map(|arb_pool_path| arb_pool_path.iter().map(|x| **x).collect())
//                     .filter(|arb_pools| self.filter_targeted_pools(arb_pools))
//                     // TBD whether this uniqueness is needed
//                     .filter(Self::path_uniqueness)
//                     .collect::<Vec<_>>()
//             })
//             .collect()
//     }
//
//     fn token_paths(
//         &self,
//         start_tokens: Vec<Address>,
//         pools_graph: &PoolsGraph,
//     ) -> HashSet<Vec<Address>> {
//         let mut token_paths = HashSet::new();
//         for start_token in start_tokens {
//             let mut stack = Vec::new();
//             stack.push((start_token, vec![start_token]));
//             while let Some((curr_item, curr_path)) = stack.pop() {
//                 if curr_path.len() as u8 == self.path_length {
//                     token_paths.insert(curr_path);
//                     continue;
//                 }
//                 let curr_item_neighbours = pools_graph.get_neighbouring_tokens(&curr_item).unwrap();
//                 if curr_path.len() as u8 == (self.path_length - 1) {
//                     if curr_item_neighbours.contains(&self.output_token) {
//                         let mut new_path = curr_path.clone();
//                         new_path.push(self.output_token);
//                         stack.push((self.output_token, new_path))
//                     }
//                     continue;
//                 }
//                 for neighbour in curr_item_neighbours.iter() {
//                     let mut new_path = curr_path.clone();
//                     new_path.push(*neighbour);
//                     stack.push((*neighbour, new_path));
//                 }
//             }
//         }
//         token_paths
//     }
//
//     fn filter_targeted_pools(&self, arb_pools: &Vec<ArbPool>) -> bool {
//         if self.pools.is_empty() {
//             return true;
//         }
//         for pool in arb_pools {
//             if self.pools.contains(&pool.pool) {
//                 return true;
//             }
//         }
//         false
//     }
//
//     fn path_uniqueness(arb_pools: &Vec<ArbPool>) -> bool {
//         let mut seen = HashSet::new();
//         for pool in arb_pools {
//             if seen.contains(&pool.pool) {
//                 return false;
//             } else {
//                 seen.insert(pool.pool);
//             }
//         }
//         true
//     }
// }
//
// pub enum ExchangeType {
//     UniswapV2,
//     UniswapV3,
// }
//
// impl From<PoolData> for ExchangeType {
//     fn from(value: PoolData) -> Self {
//         match value {
//             PoolData::UniswapV2(_) => UniswapV2,
//             PoolData::UniswapV3(_) => UniswapV3,
//         }
//     }
// }
//
// #[cfg(test)]
// mod path_builder_tests {}
