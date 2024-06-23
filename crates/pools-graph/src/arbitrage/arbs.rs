use std::cell::RefCell;
use std::collections::{BinaryHeap, HashMap};
use std::fmt::Debug;
use std::ops::Div;
use std::str::FromStr;
use std::sync::Arc;

use ethers::providers::Middleware;
use ethers::types::{Address, Block, U256, U64};
use log::error;
use revm::precompile::B256;
use revm::primitives::{alloy_primitives, ruint, AccountInfo, Bytecode, KECCAK_EMPTY};
use tokio::task::JoinSet;

use crate::arbitrage::executor::{ExecutorError, Handler};
use crate::arbitrage::ArbTx;
use crate::pool_data::pool_data::PoolDataTrait;
use crate::pool_data::uniswap_v3::utils::LoadQuoterV3;
use crate::pool_data::uniswap_v3::{QUOTER_BYTECODE, QUOTER_MOCK_ADDRESS};
use crate::pool_data::utils::EthersCacheDB;
use crate::pools_graph::PoolsGraph;

pub struct Arbs<M: Middleware, Tx, Executor> {
    // All arbitrage path. This should only contain paths that may
    // have arbitrage in a given block, except for the first block where object is built.
    // would be useful to have a zero for one concept on the paths...
    paths: Vec<Vec<ArbPool>>,
    // Heap of valid arbitrage transactions
    // they may have been computed in a previous block,but should always be valid.
    // (Tx, block_number)
    // Tuples are compared lexicographically.
    // https://stackoverflow.com/questions/61322773/how-is-ordering-defined-for-tuples-in-rust
    txs: BinaryHeap<(Tx, U64)>,
    // Used to invalidate old transactions in txs
    last_valid_tx: HashMap<Vec<ArbPool>, U64>,
    // Cache db used by REVM to compute swaps for UniswapV3, etc.
    cache_db: RefCell<EthersCacheDB<M>>,
    // Executor used to manage transaction submission to the network.
    executor: Arc<Executor>,
}

impl<M, Tx, Executor> Arbs<M, Tx, Executor>
where
    M: Middleware + 'static,
    Tx: ArbTx + Debug + Ord + Send + 'static,
    Executor: Handler<M, Tx> + Send + Sync + 'static,
{
    pub fn new(
        paths: Vec<Vec<ArbPool>>,
        cache_db: EthersCacheDB<M>,
        executor: Executor,
    ) -> Arbs<M, Tx, Executor> {
        Self {
            paths,
            txs: BinaryHeap::new(),
            last_valid_tx: HashMap::new(),
            cache_db: RefCell::new(cache_db),
            executor: Arc::new(executor),
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

    pub async fn submit_txs<T>(
        &mut self,
        pools_graph: Arc<PoolsGraph>,
        block: Block<T>,
    ) -> anyhow::Result<()> {
        let mut results = JoinSet::new();
        let next_block_base_fee = block.next_block_base_fee().unwrap();
        let block_number = block.number.unwrap();
        while let Some((mut tx, creation_block_number)) = self.txs.pop() {
            let last_valid_tx_block_number = self
                .last_valid_tx
                .get(tx.path())
                .expect("Every path should have a last valid block number.");
            // Skip arbitrage if it is old and no longer valid
            if creation_block_number < *last_valid_tx_block_number {
                continue;
            }
            let executor = Arc::clone(&self.executor);
            let graph_clone = Arc::clone(&pools_graph);
            results.spawn(async move {
                let transaction_execution = executor
                    .execute(&mut tx, &graph_clone, block_number, next_block_base_fee)
                    .await;
                (tx, transaction_execution)
            });
        }
        while let Some(result) = results.join_next().await {
            let (tx, res) = result.unwrap();
            match res {
                Ok(()) => continue,
                Err(ExecutorError::GasEstimationError(err)) => {
                    error!("{tx:#?} got a gas estimation error {err}")
                }
                Err(ExecutorError::NotEnoughProfitError | ExecutorError::LockError) => {
                    self.txs.push((tx, block_number))
                }
                Err(ExecutorError::PendingBundleError {
                    error,
                    base_fee,
                    max_priority_fee_per_gas,
                    max_fee_per_gas,
                    coinbase,
                }) => {
                    error!(
                    "Bundle was not included in target block for tx {tx:#?} because of {error:?} \n\
                    base_fee: {base_fee},\n\
                    max_priority_fee_per_gas: {max_priority_fee_per_gas},\n\
                    max_fee_per_gas: {max_fee_per_gas},\n\
                    coinbase: {coinbase}")
                }
                Err(other) => return Err(other.into()),
            }
        }
        Ok(())
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
                .compute_arbitrage(arb_path, pools_graph, max_amount_in, num_steps)
                .expect("Computed arb shouldn't fail");
            match arb_tx {
                Some(arb) => {
                    // add arb tx to heap
                    self.txs.push((arb, block_number));
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
    ) -> anyhow::Result<Option<Tx>> {
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
                let amount_out = pool_data.get_amount_out(
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
                profitable_arbs = Some(Tx::new(
                    arb_path.to_vec(),
                    tmp_amounts.iter().map(|x| x.0).collect(),
                    tmp_amounts.iter().map(|x| x.1).collect(),
                ))
            }
            amount_in += step_size;
        }
        Ok(profitable_arbs)
    }
}

impl<M, T, U> LoadQuoterV3<M> for Arbs<M, T, U>
where
    M: Middleware,
{
    fn load_uniswap_v3_quoter(&mut self) {
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

#[cfg(test)]
mod arbs_tests {
    use std::sync::Arc;

    use ethers::prelude::{Http, Provider};
    use ethers::types::{Address, U256, U64};
    use ethers::utils::Anvil;
    use revm::db::{CacheDB, EthersDB};

    use crate::arbitrage::arb_tx_v1::ArbTxV1;
    use crate::arbitrage::arbs::{ArbPool, Arbs};
    use crate::arbitrage::executor::Executor;
    use crate::arbitrage::ArbTx;
    use crate::pool_data::uniswap_v2::UniswapV2;
    use crate::pool_data::uniswap_v3::utils::LoadQuoterV3;
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

        let default_executor = Executor {
            client: provider,
            executor_address: Address::random(),
            senders: Vec::new(),
            output_token: Address::random(),
            chain_id: U64::zero(),
            tip_percentage: Default::default(),
            coinbase_threshold: U256::one(),
        };
        let mut arbs: Arbs<Provider<Http>, ArbTxV1, Executor<Provider<Http>>> =
            Arbs::new(vec![arb_path.clone()], cache_db, default_executor);

        arbs.load_uniswap_v3_quoter();

        arbs.compute_all_arbitrages(
            &pools_graph,
            U256::from_dec_str("505000000000000000000000").unwrap(),
            U256::from(100),
            block_number,
        );
        assert_eq!(arbs.txs.len(), 1);
        assert_eq!(
            arbs.txs.peek().unwrap().0.estimated_profit(),
            U256::from_dec_str("1116824102838017552066455359").unwrap()
        );
        assert_eq!(arbs.last_valid_tx.get(&arb_path).unwrap(), &block_number);
    }
}
