use std::sync::Mutex;

use ethers::prelude::{Eip1559TransactionRequest, Middleware};
use ethers::types::{Address, U64};
use thiserror::Error;

use utils::utils::FlashbotsProvider;

use crate::arbitrage::ArbTx;
use crate::pools_graph::PoolsGraph;

// Handler can be used to submit transactions of type TX to a network via
//  a given provider M.
trait Handler<M, TX>
    where
        M: Middleware,
        TX: ArbTx,
{
    async fn execute(&self, tx: TX, pools_graph: &PoolsGraph) -> Result<(), ExecutorError>;
}

#[derive(Error, Debug)]
pub enum ExecutorError {}

pub struct Executor<M: Middleware> {
    client: M,
    executor_address: Address,
    senders: Vec<Mutex<Address>>,
    output_token: Address,
    chain_id: U64,
}

impl<M: Middleware> Executor<M> {
    fn next_available_sender(&self) -> Result<Address, ExecutorError> {
        for sender in self.senders {
            match sender.try_lock() {
                Ok(addr) => { return Ok(*addr); }
                Err(err) => {}
            }
        }
        todo!()
        // for sender in self.senders {
        //     if sender.try_lock()
        // }
    }
}

impl<TX> Handler<FlashbotsProvider, TX> for Executor<FlashbotsProvider>
    where TX: ArbTx
{
    async fn execute(&self, tx: TX, pools_graph: &PoolsGraph) -> Result<(), ExecutorError> {
        let tx_bytes = tx.build_tx(pools_graph, self.executor_address, self.output_token);
        let sender = self.next_available_sender()?;
        let tx = Eip1559TransactionRequest::new();

        todo!()
    }
}
