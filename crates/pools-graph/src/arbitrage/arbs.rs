use std::cell::RefCell;
use std::collections::{BinaryHeap, HashSet};
use std::default::Default;
use std::fmt::Debug;
use std::ops::Div;
use std::str::FromStr;
use std::sync::Arc;

use ethers::providers::Middleware;
use ethers::types::{Address, Block, U256};
use log::{error, info};
use revm::precompile::B256;
use revm::primitives::{alloy_primitives, ruint, AccountInfo, Bytecode, KECCAK_EMPTY};
use tokio::task::JoinSet;

use crate::arbitrage::arb_paths::ArbPaths;
use crate::arbitrage::executor::{ExecutorError, Handler};
use crate::arbitrage::ArbTx;
use crate::pool_data::pool_data::PoolDataTrait;
use crate::pool_data::uniswap_v3::utils::LoadQuoterV3;
use crate::pool_data::uniswap_v3::{QUOTER_BYTECODE, QUOTER_MOCK_ADDRESS};
use crate::pool_data::utils::EthersCacheDB;
use crate::pools_graph::PoolsGraph;

pub struct Arbs<M: Middleware, Tx, Executor> {
    paths: ArbPaths,
    // Heap of valid arbitrage transactions
    txs: BinaryHeap<Tx>,
    // Cache db used by REVM to compute swaps for UniswapV3, etc.
    cache_db: RefCell<EthersCacheDB<M>>,
    // Executor used to manage transaction submission to the network.
    executor: Arc<Executor>,
    // min estimated profit to include in self.txs.
    min_profit: U256,
}

impl<M, Tx, Executor> Arbs<M, Tx, Executor>
where
    M: Middleware + 'static,
    Tx: ArbTx + Debug + Ord + Send + 'static,
    Executor: Handler<M, Tx> + Send + Sync + 'static,
{
    pub fn new(
        paths: ArbPaths,
        cache_db: EthersCacheDB<M>,
        executor: Executor,
        min_profit: U256,
    ) -> Arbs<M, Tx, Executor> {
        Self {
            paths,
            txs: BinaryHeap::new(),
            cache_db: RefCell::new(cache_db),
            executor: Arc::new(executor),
            min_profit,
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
        while let Some(mut tx) = self.txs.pop() {
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
                Err(ExecutorError::GasEstimationError(_)) => {}
                Err(ExecutorError::NotEnoughProfitError) => {
                    info!("Profit is too low at the moment for {tx:?}");
                    self.txs.push(tx)
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
        targeted_pools: &[Address],
        pools_graph: &PoolsGraph,
        max_amount_in: U256,
        num_steps: U256,
    ) {
        let mut paths = Vec::new();
        for targeted_pool in targeted_pools {
            if let Some(p) = self.paths.get_paths(targeted_pool) {
                for path in p {
                    paths.push(path.as_ref());
                }
            }
        }

        let mut seen = HashSet::new();
        // this can probably be parallelized by making clones of cache db for each arb path..
        // This might be quicker but not sure.
        // Needs to be benchmarked.
        for arb_path in &paths {
            if seen.contains(arb_path) {
                continue;
            }
            seen.insert(*arb_path);
            // info!("Computing arb for path {arb_path:#?}");
            match self.compute_arbitrage(arb_path, pools_graph, max_amount_in, num_steps) {
                Ok(Some(arb)) => {
                    if arb.estimated_profit() > self.min_profit {
                        info!("Adding arb {arb:#?} to heap");
                        self.txs.push(arb)
                    }
                }
                Ok(None) => (),
                Err(err) => error!("An error occured for path {arb_path:#?} ---> {err}"),
            }
        }
    }

    fn compute_arbitrage(
        &self,
        arb_path: &[ArbPool],
        pools_graph: &PoolsGraph,
        max_amount_in: U256,
        num_steps: U256,
    ) -> anyhow::Result<Option<Tx>> {
        // Start at 1 because 0 seems to cause an error.
        let mut amount_in = U256::one();
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
                    prev_amount_in,
                    arb_pool.token_in,
                    arb_pool.token_out,
                    Some(&mut self.cache_db.borrow_mut()),
                )?;
                tmp_amounts.push((prev_amount_in, amount_out));
                prev_amount_in = amount_out;
            }

            if prev_amount_in > amount_in && prev_amount_in - amount_in > curr_max_profit {
                // println!("{tmp_amounts:#?}");
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

    use crate::arbitrage::arb_paths::ArbPaths;
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
            sender: Address::random(),
            output_token: Address::random(),
            chain_id: U64::zero(),
            tip_percentage: Default::default(),
            coinbase_threshold: U256::one(),
        };

        let mut arb_paths = ArbPaths::default();
        arb_paths
            .insert_path(arb_path.clone())
            .expect("Should insert fine");
        let mut arbs: Arbs<Provider<Http>, ArbTxV1, Executor<Provider<Http>>> =
            Arbs::new(arb_paths, cache_db, default_executor, U256::zero());

        arbs.load_uniswap_v3_quoter();

        arbs.compute_all_arbitrages(
            &[uniswap_v3_pool.pool_address, uniswap_v2_pair.pair_address],
            &pools_graph,
            U256::from_dec_str("505000000000000000000000").unwrap(),
            U256::from(10),
        );
        assert_eq!(arbs.txs.len(), 1);
        assert_eq!(
            arbs.txs.peek().unwrap().estimated_profit(),
            U256::from_dec_str("6556500114824717164690").unwrap()
        );
    }
}
