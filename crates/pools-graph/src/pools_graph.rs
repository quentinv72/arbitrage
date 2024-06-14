use std::collections::HashSet;
use std::sync::Arc;

use dashmap::mapref::one::{Ref, RefMut};
use dashmap::try_result::TryResult;
use dashmap::{DashMap, DashSet};
use ethers::contract::ContractError;
use ethers::prelude::TxHash;
use ethers::providers::Middleware;
use ethers::types::{Address, Block};

use crate::pool_data::factory::{Factory, GetAddress};
use crate::pool_data::pool_data::{PoolData, PoolDataTrait};

#[derive(Default)]
pub struct PoolsGraph {
    pool_address_to_pool_data: DashMap<Address, PoolData>,
    // ERC-20 token -> Set<ERC-20 token>
    neighbouring_erc20_tokens: DashMap<Address, DashSet<Address>>,
    // (ERC-20 token, ERC-20 token) -> Pool Address
    weights: DashMap<(Address, Address), DashSet<Address>>,
    // Set of all factories addresses
    factories: DashMap<Address, Factory>,
}

impl PoolsGraph {
    #[inline]
    pub fn get_pool_data(&self, pool_address: &Address) -> Option<Ref<Address, PoolData>> {
        self.pool_address_to_pool_data.get(pool_address)
    }

    #[inline]
    pub fn get_neighbouring_tokens(
        &self,
        token_address: &Address,
    ) -> Option<Ref<Address, DashSet<Address>>> {
        self.neighbouring_erc20_tokens.get(token_address)
    }

    #[inline]
    pub fn get_pool_addresses(
        &self,
        token_0: Address,
        token_1: Address,
    ) -> Option<Ref<(Address, Address), DashSet<Address>>> {
        self.weights.get(&(token_0, token_1))
    }

    // TODO figure out how to reduce number of calls to get logs.
    // Maybe using eth_getLogs?
    // Use different ethereum client
    #[inline]
    pub async fn maybe_update_graph<Tx, M>(
        &self,
        block: Block<Tx>,
        client: Arc<M>,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        Tx: Into<TxHash> + Send + Sync,
        M: Middleware + 'static,
    {
        let mut pools_to_update = HashSet::new();
        for tx in block.transactions {
            self.maybe_update_graph_tx(tx, &mut pools_to_update, client.clone())
                .await?;
        }
        self.flush_updates(client, pools_to_update).await?;
        Ok(())
    }

    #[inline]
    async fn maybe_update_graph_tx<Tx, M>(
        &self,
        tx: Tx,
        pools_to_update: &mut HashSet<Address>,
        client: Arc<M>,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        Tx: Into<TxHash> + Send + Sync,
        M: Middleware + 'static,
    {
        let tx_receipt = client
            .get_transaction_receipt(tx)
            .await?
            .unwrap_or_default();
        for log in tx_receipt.logs {
            if self.pool_address_to_pool_data.contains_key(&log.address) {
                pools_to_update.insert(log.address);
            } else if self.factories.contains_key(&log.address) {
                let factory = self.factories.get(&log.address).unwrap();
                if let Some(new_pool) = PoolData::new_pool(log, *factory)? {
                    pools_to_update.insert(new_pool.get_pool_address());
                    self.insert(new_pool);
                }
            }
        }
        Ok(())
    }

    #[inline]
    // TODO figure out if this can be parallelized
    // intuition is probably not...
    async fn flush_updates<M: Middleware + 'static>(
        &self,
        client: Arc<M>,
        pools_to_update: HashSet<Address>,
    ) -> Result<(), ContractError<M>> {
        for pool in pools_to_update.iter() {
            let client_clone = Arc::clone(&client);
            let mut pool_data = self.pool_address_to_pool_data.get_mut(pool).unwrap();
            pool_data.update_pool(client_clone).await?;
        }
        Ok(())
    }

    pub(crate) fn get_mut_pool_data(
        &self,
        pool_address: &Address,
    ) -> TryResult<RefMut<Address, PoolData>> {
        self.pool_address_to_pool_data.try_get_mut(pool_address)
    }

    pub fn insert(&self, pool_data: PoolData) {
        let factory = pool_data.get_factory();
        let (token_0, token_1) = pool_data.get_tokens();
        let pool_address = pool_data.get_pool_address();
        self.pool_address_to_pool_data
            .insert(pool_address, pool_data);
        self.insert_tokens(pool_address, token_0, token_1);
        self.insert_tokens(pool_address, token_1, token_0);
        if !self.factories.contains_key(&factory.get_address()) {
            self.factories.insert(factory.get_address(), factory);
        }
    }

    pub(crate) fn get_all_tokens(&self) -> Vec<Address> {
        self.neighbouring_erc20_tokens
            .iter()
            .map(|x| *x.key())
            .collect()
    }

    fn insert_tokens(&self, pool_address: Address, token_0: Address, token_1: Address) {
        match self.neighbouring_erc20_tokens.get(&token_0) {
            Some(kv) => {
                kv.value().insert(token_1);
                match self.weights.get(&(token_0, token_1)) {
                    Some(kv) => {
                        kv.value().insert(pool_address);
                    }
                    None => {
                        let weight_set = DashSet::new();
                        weight_set.insert(pool_address);
                        self.weights.insert((token_0, token_1), weight_set);
                    }
                }
            }
            None => {
                let neighbour_set = DashSet::new();
                neighbour_set.insert(token_1);
                self.neighbouring_erc20_tokens
                    .insert(token_0, neighbour_set);
                let weight_set = DashSet::new();
                weight_set.insert(pool_address);
                self.weights.insert((token_0, token_1), weight_set);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ethers::prelude::{Http, Provider};
    use ethers::providers::Middleware;
    use ethers::types::{Address, BlockNumber, U256, U64};
    use ethers::utils::Anvil;

    use crate::pool_data::factory::FactoryV2;
    use crate::pool_data::pool_data::{PoolData, PoolDataTrait};
    use crate::pool_data::uniswap_v2::UniswapV2;
    use crate::pool_data::uniswap_v3::UniswapV3;
    use crate::pools_graph::PoolsGraph;

    #[test]
    fn successfully_inserts_into_graph() {
        let graph: PoolsGraph = Default::default();
        let token_a = "0x61fFE014bA17989E743c5F6cB21bF9697530B21e"
            .parse()
            .unwrap();
        let token_b = "0xbACEB8eC6b9355Dfc0269C18bac9d6E2Bdc29C4F"
            .parse()
            .unwrap();
        let token_c = "0xbACEB8eC6b9355Dfc0269C18bac9d6E2Bdc29D4F"
            .parse()
            .unwrap();
        let address_1 = "0x1F98431c8aD98523631AE4a59f267346ea31F984"
            .parse()
            .unwrap();
        let address_2 = "0x1F98431c8aD98523631AE4a59f267346ea31F784"
            .parse()
            .unwrap();
        let pd_1 = UniswapV3::new(address_1, U256::from(0), token_a, token_b, 1000, None);

        let pd_2 = UniswapV2::new(
            address_2,
            token_a,
            1999,
            token_c,
            939393,
            ethers::prelude::U64::zero(),
            None,
        );

        graph.insert(pd_1.into());
        graph.insert(pd_2.into());

        assert_eq!(graph.factories.len(), 2);

        assert!(graph
            .neighbouring_erc20_tokens
            .get(&token_a)
            .unwrap()
            .contains(&token_b));
        assert!(graph
            .neighbouring_erc20_tokens
            .get(&token_a)
            .unwrap()
            .contains(&token_c));
        assert!(graph
            .neighbouring_erc20_tokens
            .get(&token_c)
            .unwrap()
            .contains(&token_a));
        assert!(graph
            .neighbouring_erc20_tokens
            .get(&token_b)
            .unwrap()
            .contains(&token_a));
        assert!(!graph
            .neighbouring_erc20_tokens
            .get(&token_b)
            .unwrap()
            .contains(&token_c));

        assert!(graph
            .weights
            .get(&(token_a, token_b))
            .unwrap()
            .contains(&address_1));
        assert!(graph
            .weights
            .get(&(token_b, token_a))
            .unwrap()
            .contains(&address_1));
        assert_eq!(graph.weights.get(&(token_b, token_a)).unwrap().len(), 1);
        assert_eq!(graph.weights.get(&(token_a, token_b)).unwrap().len(), 1);
        assert!(graph
            .weights
            .get(&(token_a, token_c))
            .unwrap()
            .contains(&address_2));

        assert!(graph
            .weights
            .get(&(token_c, token_a))
            .unwrap()
            .contains(&address_2));
        assert_eq!(graph.weights.get(&(token_a, token_c)).unwrap().len(), 1);
        assert_eq!(graph.weights.get(&(token_c, token_a)).unwrap().len(), 1);
        assert_eq!(
            graph
                .pool_address_to_pool_data
                .get(&address_1)
                .unwrap()
                .value()
                .get_pool_address(),
            address_1
        );
        assert_eq!(
            graph
                .pool_address_to_pool_data
                .get(&address_2)
                .unwrap()
                .value()
                .get_pool_address(),
            address_2
        );

        assert!(graph.get_pool_data(&address_1).is_some());
        assert!(graph.get_pool_data(&token_c).is_none());
        assert!(graph
            .get_neighbouring_tokens(&token_a)
            .unwrap()
            .contains(&token_c));
        assert!(graph
            .get_neighbouring_tokens(&token_c)
            .unwrap()
            .contains(&token_a));
        assert!(graph
            .get_pool_addresses(token_a, token_b)
            .unwrap()
            .contains(&address_1));
        assert!(graph.get_pool_addresses(token_a, token_a).is_none());
    }

    #[tokio::test]
    async fn graph_update() {
        let graph = PoolsGraph::default();
        let factory_address = "0xB7f907f7A9eBC822a80BD25E224be42Ce0A698A0"
            .parse()
            .unwrap();
        let factory = FactoryV2 {
            address: factory_address,
            swap_fee: U256::from(3),
        };
        graph.factories.insert(factory_address, factory.into());
        let anvil = Anvil::new()
            .fork("https://eth-sepolia.g.alchemy.com/v2/fEmCuDGqB-tSA4R5HnnVCy1n9Jg4GqJg@6011385")
            .spawn();
        let new_pair_address: Address = "0xDaB5F999db06e852d5F6C5e24F00DeF232B8B189"
            .parse()
            .unwrap();
        let provider = Arc::new(Provider::<Http>::try_from(anvil.endpoint()).unwrap());
        assert!(graph.get_pool_data(&new_pair_address).is_none());
        let block = provider.get_block(BlockNumber::Latest).await.unwrap();
        graph
            .maybe_update_graph(block.unwrap(), provider)
            .await
            .unwrap();
        assert_eq!(
            *graph.get_pool_data(&new_pair_address).unwrap().value(),
            PoolData::UniswapV2(UniswapV2::new(
                new_pair_address,
                "0xcB86e0b626FB47c8017E92d0b41B1b29F267aDAC"
                    .parse()
                    .unwrap(),
                0,
                "0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14"
                    .parse()
                    .unwrap(),
                0,
                U64::zero(),
                Some(factory),
            ))
        );
        drop(anvil);
    }
}
