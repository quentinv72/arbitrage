use std::ops::Mul;
use std::sync::Mutex;

use ethers::core::k256::ecdsa::SigningKey;
use ethers::middleware::signer::SignerMiddlewareError;
use ethers::prelude::*;
use ethers::types::{Address, U256, U64};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers_flashbots::{BundleRequest, PendingBundleError};
use log::info;
use thiserror::Error;

use utils::utils::FlashbotsProvider;

use crate::arbitrage::ArbTx;
use crate::arbitrage::executor::ExecutorError::LockError;
use crate::pools_graph::PoolsGraph;

// Handler can be used to submit transactions of type TX to a network via
//  a given provider M.
trait Handler<M, TX>
    where
        M: Middleware,
        TX: ArbTx,
{
    async fn execute(
        &self,
        tx: &mut TX,
        pools_graph: &PoolsGraph,
        block_number: U64,
        next_block_base_fee: U256,
    ) -> Result<(), ExecutorError<M>>;
}

#[derive(Error, Debug)]
pub enum ExecutorError<M: Middleware> {
    #[error("All addresses are locked")]
    LockError,

    #[error(transparent)]
    GasEstimationError(#[from] SignerMiddlewareError<M::Inner, Wallet<SigningKey>>),

    #[error(transparent)]
    WalletError(#[from] WalletError),

    #[error("Transaction does not generate high enough profits to submit to network")]
    NotEnoughProfitError,

    #[error("Error from flashbots middleware: {0}")]
    FlashbotsMiddlewareError(String),

    #[error(transparent)]
    PendingBundleError(#[from] PendingBundleError),

    #[error(transparent)]
    OtherError(#[from] anyhow::Error),
}

pub struct Executor<M: Middleware> {
    client: M,
    executor_address: Address,
    senders: Vec<Mutex<Address>>,
    output_token: Address,
    chain_id: U64,
    priority_fee_percentage: u32,
    coinbase_threshold: U256,
}

impl<M: Middleware> Executor<M> {
    fn next_available_sender(&self) -> Result<Address, ExecutorError<M>> {
        for sender in &self.senders {
            match sender.try_lock() {
                Ok(addr) => {
                    return Ok(*addr);
                }
                Err(_) => {
                    continue;
                }
            }
        }
        return Err(LockError);
    }
}

impl<TX> Handler<FlashbotsProvider, TX> for Executor<FlashbotsProvider>
    where
        TX: ArbTx,
{
    async fn execute(
        &self,
        tx: &mut TX,
        pools_graph: &PoolsGraph,
        block_number: U64,
        next_block_base_fee: U256,
    ) -> Result<(), ExecutorError<FlashbotsProvider>> {
        let tx_bytes = tx.get_bytes(pools_graph, self.executor_address, self.output_token);
        let sender = self.next_available_sender()?;
        let mut eip_1559_tx = Eip1559TransactionRequest::new()
            .data(tx_bytes)
            .chain_id(self.chain_id)
            .from(sender)
            .to(self.executor_address);
        let gas_estimate = self
            .client
            .estimate_gas(&(eip_1559_tx.clone().into()), None)
            .await?;
        if gas_estimate.mul(next_block_base_fee) >= tx.estimated_profit() {
            return Err(ExecutorError::<FlashbotsProvider>::NotEnoughProfitError.into());
        }

        eip_1559_tx = eip_1559_tx.gas(gas_estimate);
        let mut effective_profit = gas_estimate.mul(next_block_base_fee) - tx.estimated_profit();
        if effective_profit > self.coinbase_threshold {
            todo!("Calculate coinbase");
            todo!("Calculate priority fees and set them on eip tx");
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
            if let Some(bundle_hash) = pending_bundle.await? {
                info!("Bundle with hash {bundle_hash:?} was included in target block")
            }
        }
        Ok(())
    }
}
