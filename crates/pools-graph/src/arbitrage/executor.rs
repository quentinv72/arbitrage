use std::fmt::Debug;
use std::future::Future;
use std::ops::{Div, Mul};
use std::sync::Arc;

use ethers::core::k256::ecdsa::SigningKey;
use ethers::middleware::signer::SignerMiddlewareError;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, U256, U64};
use ethers_flashbots::{BundleRequest, PendingBundleError};
use log::{debug, info};
use thiserror::Error;

use utils::utils::FlashbotsProvider;

use crate::arbitrage::ArbTx;
use crate::pools_graph::PoolsGraph;

// Handler can be used to submit transactions of type TX to a network via a given provider M.
pub trait Handler<M, TX>
where
    M: Middleware,
    TX: ArbTx,
{
    fn execute(
        &self,
        tx: &mut TX,
        pools_graph: &PoolsGraph,
        block_number: U64,
        next_block_base_fee: U256,
    ) -> impl Future<Output = Result<(), ExecutorError<M>>> + Send;
}

#[derive(Error, Debug)]
pub enum ExecutorError<M: Middleware> {
    // Error occurred during gas estimation.
    #[error(transparent)]
    GasEstimationError(#[from] SignerMiddlewareError<M::Inner, Wallet<SigningKey>>),

    // Error occurring when signing tx.
    #[error(transparent)]
    WalletError(#[from] WalletError),

    // TX is not profitable enough after calculating gas fees.
    #[error("Transaction does not generate high enough profits to submit to network")]
    NotEnoughProfitError,

    // Flashbots middleware error. Shouldnt be here but cant figure out where esle to put it.
    #[error("Error from flashbots middleware: {0}")]
    FlashbotsMiddlewareError(String),

    // Pending Flashbot bundle error. Shouldn't be here either, but can't figure out
    // where else to put it.
    #[error("Bundle was not included")]
    PendingBundleError {
        error: PendingBundleError,
        base_fee: U256,
        max_priority_fee_per_gas: U256,
        max_fee_per_gas: U256,
        coinbase: U256,
    },
}

#[derive(Default)]
pub struct Executor<M> {
    // Client to connect to the network.
    pub client: Arc<M>,
    // Address of the contract that will execute the transaction.
    pub executor_address: Address,
    // List of addresses that are authorized to call the `self.executor_address`.
    // They are guarded by a mutex to avoid having to manage nonces. Each sender can only
    // send 1 tx per block.
    pub sender: Address,
    // Token that is sent back to the contract owner at the end of the transaction.
    pub output_token: Address,
    // Chain ID.
    pub chain_id: U64,
    // Percentage of the profit that should be as tip. Must be a value between 0 and 10_000.
    pub tip_percentage: u32,
    // Threshold at which the transaction should send coinbase to the validator rather than using
    // priority fee field on the request. This is mostly for transactions
    // that are highly profitable.
    pub coinbase_threshold: U256,
}

impl<TX> Handler<FlashbotsProvider, TX> for Executor<FlashbotsProvider>
where
    TX: ArbTx + Send + Sync + Debug,
{
    async fn execute(
        &self,
        tx: &mut TX,
        pools_graph: &PoolsGraph,
        block_number: U64,
        next_block_base_fee: U256,
    ) -> Result<(), ExecutorError<FlashbotsProvider>> {
        let tx_bytes = tx.get_bytes(pools_graph, self.executor_address, self.output_token);
        let mut eip_1559_tx = Eip1559TransactionRequest::new()
            .data(tx_bytes)
            .chain_id(self.chain_id)
            .from(self.sender)
            .to(self.executor_address);
        debug!("{tx:#?}");
        debug!("{eip_1559_tx:?}");
        let gas_estimate = self
            .client
            .estimate_gas(&(eip_1559_tx.clone().into()), None)
            .await?;
        if gas_estimate.mul(next_block_base_fee) >= tx.estimated_profit() {
            return Err(ExecutorError::<FlashbotsProvider>::NotEnoughProfitError);
        }

        eip_1559_tx = eip_1559_tx.gas(gas_estimate);
        let profit = tx.estimated_profit() - gas_estimate.mul(next_block_base_fee);
        let tip = profit
            .mul(U256::from(self.tip_percentage))
            .div(U256::from(10_000));

        // Set coinbase if not already set and update EIP1559 transaction
        if profit > self.coinbase_threshold && tx.get_amount_to_coinbase().is_zero() {
            tx.set_amount_to_coinbase(tip);
            eip_1559_tx = eip_1559_tx
                .data(tx.get_bytes(pools_graph, self.executor_address, self.output_token))
                .max_priority_fee_per_gas(U256::zero())
                .max_fee_per_gas(next_block_base_fee);
        }

        // If coinbase is zero, pay tip as part of tx fees
        if tx.get_amount_to_coinbase().is_zero() {
            let tip_per_gas = tip.div(gas_estimate);
            eip_1559_tx = eip_1559_tx
                .max_priority_fee_per_gas(tip_per_gas)
                .max_fee_per_gas(tip_per_gas + next_block_base_fee);
        }

        let typed_tx: TypedTransaction = eip_1559_tx.into();
        let signature = self.client.signer().sign_transaction(&typed_tx).await?;
        let bundle = BundleRequest::new()
            .push_transaction(typed_tx.rlp_signed(&signature))
            .set_block(block_number + 1)
            .set_simulation_block(block_number)
            .set_simulation_timestamp(0);
        let results = self
            .client
            .inner()
            .send_bundle(&bundle)
            .await
            .map_err(|err| ExecutorError::FlashbotsMiddlewareError(format!("{err}")))?;
        for result in results {
            let pending_bundle =
                result.map_err(|err| ExecutorError::FlashbotsMiddlewareError(format!("{err}")))?;
            let bundle_hash =
                pending_bundle
                    .await
                    .map_err(|err| ExecutorError::PendingBundleError {
                        error: err,
                        base_fee: next_block_base_fee,
                        max_priority_fee_per_gas: typed_tx
                            .as_eip1559_ref()
                            .unwrap()
                            .max_priority_fee_per_gas
                            .unwrap(),
                        max_fee_per_gas: typed_tx
                            .as_eip1559_ref()
                            .unwrap()
                            .max_fee_per_gas
                            .unwrap(),
                        coinbase: tx.get_amount_to_coinbase(),
                    })?;
            if let Some(b_hash) = bundle_hash {
                info!("Bundle with hash {b_hash:?} was included in target block")
            }
        }
        Ok(())
    }
}

impl<TX> Handler<Provider<Http>, TX> for Executor<Provider<Http>>
where
    TX: ArbTx + Send + Sync,
{
    async fn execute(
        &self,
        _tx: &mut TX,
        _pools_graph: &PoolsGraph,
        _block_number: U64,
        _next_block_base_fee: U256,
    ) -> Result<(), ExecutorError<Provider<Http>>> {
        todo!()
    }
}
