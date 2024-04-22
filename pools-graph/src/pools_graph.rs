use dashmap::{DashMap, DashSet};
use dashmap::mapref::one::Ref;
use ethers::types::Address;

use crate::pool_data::pool_data::PoolData;

pub struct PoolsGraph {
    _pool_address_to_pool_data: DashMap<Address, Box<dyn PoolData>>,
    // ERC-20 token -> Set<ERC-20 token>
    _neighbouring_erc20_tokens: DashMap<Address, DashSet<Address>>,
    // (ERC-20 token, ERC-20 token) -> Pool Address
    _weights: DashMap<(Address, Address), DashSet<Address>>,
}

impl PoolsGraph {
    pub fn new() -> Self {
        Self {
            _pool_address_to_pool_data: DashMap::new(),
            _weights: DashMap::new(),
            _neighbouring_erc20_tokens: DashMap::new(),
        }
    }

    pub fn get_pool_data(&self, pool_address: &Address) -> Option<Ref<Address, Box<dyn PoolData>>> {
        self._pool_address_to_pool_data.get(pool_address)
    }

    pub fn get_neighbouring_tokens(&self, token_address: &Address) -> Option<Ref<Address, DashSet<Address>>> {
        self._neighbouring_erc20_tokens.get(token_address)
    }

    pub fn get_pool_addresses(&self, token_0: Address, token_1: Address) -> Option<Ref<(Address, Address), DashSet<Address>>> {
        self._weights.get(&(token_0, token_1))
    }

    pub(crate) fn insert(&self, pool_data: Box<dyn PoolData>) {
        let (token_0, token_1) = pool_data.get_tokens();
        let pool_address = pool_data.get_pool_address();
        self._pool_address_to_pool_data.insert(pool_address, pool_data);
        self.insert_tokens(pool_address, token_0, token_1);
        self.insert_tokens(pool_address, token_1, token_0);
    }

    fn insert_tokens(&self, pool_address: Address, token_0: Address, token_1: Address) {
        match self._neighbouring_erc20_tokens.get(&token_0) {
            Some(kv) => {
                kv.value().insert(token_1);
                match self._weights.get(&(token_0, token_1)) {
                    Some(kv) => {
                        kv.value().insert(pool_address);
                        ()
                    }
                    None => {
                        let weight_set = DashSet::new();
                        weight_set.insert(pool_address);
                        self._weights.insert((token_0, token_1), weight_set);
                    }
                }
            }
            None => {
                let neighbour_set = DashSet::new();
                neighbour_set.insert(token_1);
                self._neighbouring_erc20_tokens.insert(token_0, neighbour_set);
                let weight_set = DashSet::new();
                weight_set.insert(pool_address);
                self._weights.insert((token_0, token_1), weight_set);
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use ethers::types::U256;

    use crate::pool_data::uniswap_v2::UniswapV2;
    use crate::pool_data::uniswap_v3::UniswapV3;
    use crate::pools_graph::PoolsGraph;

    #[test]
    fn successfully_inserts_into_graph() {
        let graph = PoolsGraph::new();
        let quoter_address = "0x1F98431c8aD98553881AE4a59f267346ea31F784";
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
        let pd_1 = Box::new(UniswapV3::new(
            address_1,
            quoter_address.parse().unwrap(),
            U256::from(0),
            token_a,
            token_b,
            1000,
        ));

        let pd_2 = Box::new(UniswapV2::new(
            address_2,
            token_a,
            1999,
            token_c,
            939393,
            ethers::prelude::U64::zero(),
        ));

        graph.insert(pd_1);
        graph.insert(pd_2);

        assert!(graph._neighbouring_erc20_tokens.get(&token_a).unwrap().contains(&token_b));
        assert!(graph._neighbouring_erc20_tokens.get(&token_a).unwrap().contains(&token_c));
        assert!(graph._neighbouring_erc20_tokens.get(&token_c).unwrap().contains(&token_a));
        assert!(graph._neighbouring_erc20_tokens.get(&token_b).unwrap().contains(&token_a));
        assert!(!graph._neighbouring_erc20_tokens.get(&token_b).unwrap().contains(&token_c));

        assert!(graph
            ._weights
            .get(&(token_a, token_b))
            .unwrap().contains(&address_1));
        assert!(graph
            ._weights
            .get(&(token_b, token_a))
            .unwrap()
            .contains(&address_1));
        assert_eq!(graph._weights.get(&(token_b, token_a)).unwrap().len(), 1);
        assert_eq!(graph._weights.get(&(token_a, token_b)).unwrap().len(), 1);
        assert!(graph
            ._weights
            .get(&(token_a, token_c))
            .unwrap()
            .contains(&address_2));

        assert!(graph
            ._weights
            .get(&(token_c, token_a))
            .unwrap()
            .contains(&address_2));
        assert_eq!(graph._weights.get(&(token_a, token_c)).unwrap().len(), 1);
        assert_eq!(graph._weights.get(&(token_c, token_a)).unwrap().len(), 1);
        assert_eq!(
            graph
                ._pool_address_to_pool_data
                .get(&address_1)
                .unwrap()
                .value().get_pool_address(),
            address_1
        );
        assert_eq!(
            graph
                ._pool_address_to_pool_data
                .get(&address_2)
                .unwrap()
                .value().get_pool_address(),
            address_2
        );

        assert!(graph.get_pool_data(&address_1).is_some());
        assert!(graph.get_pool_data(&token_c).is_none());
        assert!(graph.get_neighbouring_tokens(&token_a).unwrap().contains(&token_c));
        assert!(graph.get_neighbouring_tokens(&token_c).unwrap().contains(&token_a));
        assert!(graph.get_pool_addresses(token_a, token_b).unwrap().contains(&address_1));
        assert!(graph.get_pool_addresses(token_a, token_a).is_none());
    }
}