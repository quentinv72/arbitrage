use std::cmp::Ordering;
use std::collections::HashSet;
use std::ops::{Div, Mul};
use std::sync::Arc;

use ethers::abi::{encode, Token};
use ethers::contract::ContractCall;
use ethers::prelude::*;
use ethers::providers::Middleware;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, Bytes, U256, U64};
use ethers_flashbots::*;
use log::{error, info, warn};

use contracts::qv_executor::QVExecutor;
use utils::utils::FlashbotsProvider;

use crate::pool_data::pool_data::PoolDataTrait;
use crate::pools_graph::PoolsGraph;

#[derive(Eq, Debug)]
pub struct Arbitrage {
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
    pub fn new(
        targets: Vec<Address>,
        amounts_in: Vec<U256>,
        amounts_out: Vec<U256>,
        zero_for_ones: Vec<bool>,
        amount_to_coinbase: U256,
        profit: Option<U256>,
    ) -> Arbitrage {
        let estimated_profit = match profit {
            None => amounts_out.last().unwrap() - amounts_in.first().unwrap(),
            Some(val) => val,
        };

        Arbitrage {
            targets,
            amounts_in,
            amounts_out,
            zero_for_ones,
            amount_to_coinbase,
            estimated_profit,
        }
    }

    pub fn get_estimated_profit(&self) -> U256 {
        self.estimated_profit
    }

    pub fn get_pair_addresses(&self) -> &Vec<Address> {
        &self.targets
    }

    fn build_transaction<M: Middleware>(
        &self,
        pools_graph: &PoolsGraph,
        output_token: Address,
        client: Arc<M>,
        bundle_executor_address: Address,
    ) -> ContractCall<M, ()> {
        let mut next_call = Self::build_data(
            *self.amounts_in.last().unwrap(),
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
                bundle_executor_address,
            );
            next_call = Self::build_data(self.amounts_in[i - 1], self.targets[i], swap_data);
        }

        let start_pool_addr = self.targets[0];
        let start_pool = pools_graph.get_pool_data(&start_pool_addr).unwrap();
        next_call = start_pool.build_swap_calldata(
            self.amounts_in[0],
            self.amounts_out[0],
            self.zero_for_ones[0],
            next_call,
            Arc::clone(&client),
            bundle_executor_address,
        );
        let bundle_executor_contract = QVExecutor::new(bundle_executor_address, client);
        bundle_executor_contract.execute_bundle(
            self.targets[0],
            self.amount_to_coinbase,
            output_token,
            next_call,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn submit_transaction_flashbots(
        &self,
        graph: &PoolsGraph,
        output_token: Address,
        bundle_executor_address: Address,
        next_block_base_fee: U256,
        has_reverted: &mut HashSet<Vec<Address>>,
        client: Arc<FlashbotsProvider>,
        priority_fee_percentage: u32,
        block_number: U64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cc = self.build_transaction(
            graph,
            output_token,
            Arc::clone(&client),
            bundle_executor_address,
        );

        let gas_estimate_opt = match cc.estimate_gas().await {
            Ok(est) => Some(est),
            Err(e) => {
                if e.is_revert() {
                    if !has_reverted.contains(self.get_pair_addresses()) {
                        warn!(
                            "Gas estimate reverted for pairs {:#?} \nThe calldata {:#?}",
                            self.get_pair_addresses(),
                            cc.tx.data()
                        );
                        has_reverted.insert(self.get_pair_addresses().to_vec());
                    }
                } else {
                    panic!("Got a strange error {e}")
                }
                None
            }
        };

        if gas_estimate_opt.is_none() {
            return Ok(());
        }
        let gas_estimate = gas_estimate_opt.unwrap();

        if gas_estimate.mul(next_block_base_fee) < self.estimated_profit {
            let client_clone = Arc::clone(&client);
            let estimated_profit = self.estimated_profit;
            tokio::spawn(async move {
                match Self::try_submit_trade_flashbots(
                    estimated_profit,
                    cc.tx,
                    gas_estimate,
                    next_block_base_fee,
                    client_clone,
                    priority_fee_percentage,
                    block_number,
                )
                .await
                {
                    Ok(_) => (),
                    Err(e) => {
                        error!("Failed to submit trade... see error {e}");
                    }
                };
            });
        }
        Ok(())
    }

    async fn try_submit_trade_flashbots(
        estimated_profit: U256,
        tx: TypedTransaction,
        gas_estimate: U256,
        base_fee: U256,
        rpc_client: Arc<FlashbotsProvider>,
        priority_fee_percentage: u32,
        block_number: U64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "Found a trade with estimated profit of {}",
            estimated_profit
        );
        let remaining_profit = estimated_profit - gas_estimate.mul(base_fee);
        let max_priority_fee_per_gas = (remaining_profit.div(gas_estimate))
            .mul(U256::from(priority_fee_percentage))
            .div(U256::from(10_000));
        let max_fee = base_fee + max_priority_fee_per_gas;
        match tx {
            TypedTransaction::Eip1559(inner) => {
                let tx: TypedTransaction = inner
                    .gas(gas_estimate)
                    .max_priority_fee_per_gas(max_priority_fee_per_gas)
                    .max_fee_per_gas(max_fee)
                    .chain_id(1)
                    .into();
                let signature = rpc_client.signer().sign_transaction(&tx).await?;
                let bundle = BundleRequest::new()
                    .push_transaction(tx.rlp_signed(&signature))
                    .set_block(block_number + 1)
                    .set_simulation_block(block_number)
                    .set_simulation_timestamp(0);

                // Simulate it
                // let simulated_bundle = rpc_client.inner().simulate_bundle(&bundle).await?;
                // info!("Simulated bundle: {:#?}", simulated_bundle);
                // Send it
                let results = rpc_client.inner().send_bundle(&bundle).await?;
                for result in results {
                    match result {
                        Ok(pending_bundle) => match pending_bundle.await {
                            Ok(bundle_hash) => info!(
                                "Bundle with hash {:?} was included in target block",
                                bundle_hash
                            ),
                            Err(PendingBundleError::BundleNotIncluded) => {
                                error!("Bundle was not included in target block.")
                            }
                            Err(e) => error!("An error occured: {}", e),
                        },
                        Err(e) => error!("An error occured: {}", e),
                    }
                }
                Ok(())
            }
            _other => panic!("Uggh this should be EIP1559"),
        }
    }

    fn build_data(amount_in: U256, target_address: Address, swap_data: Bytes) -> Bytes {
        Bytes::from(encode(&[
            Token::Uint(amount_in),
            Token::Address(target_address),
            Token::Bytes(swap_data.to_vec()),
        ]))
    }
}
