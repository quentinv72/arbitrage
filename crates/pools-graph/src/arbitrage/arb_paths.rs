use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ethers::types::Address;
use thiserror::Error;

use crate::arbitrage::arb_paths::ArbPathsErrors::DuplicatedPools;
use crate::arbitrage::arbs::ArbPool;

#[derive(Default)]
pub struct ArbPaths {
    // Maps a pool identifier to a list of arbitrage paths. The paths are wrapped in an Rc<_> so
    // that we don't need to clone each Vec multiple times.
    pool_to_paths: HashMap<Address, Vec<Rc<Vec<ArbPool>>>>,
}

#[derive(Error, Debug)]
pub enum ArbPathsErrors {
    #[error("Duplicated pools in path")]
    DuplicatedPools,
}

impl ArbPaths {
    #[inline]
    pub fn get_paths(&self, addr: &Address) -> Option<&Vec<Rc<Vec<ArbPool>>>> {
        self.pool_to_paths.get(addr)
    }

    #[inline]
    pub fn insert_path(&mut self, path: Vec<ArbPool>) -> Result<(), ArbPathsErrors> {
        let mut seen = HashSet::new();
        for pool in &path {
            if seen.contains(&pool.pool) {
                return Err(DuplicatedPools);
            }
            seen.insert(pool.pool);
        }

        let rc_path = Rc::new(path);
        for arb_pool in rc_path.iter() {
            let key = arb_pool.pool;
            match self.pool_to_paths.entry(key) {
                Entry::Occupied(mut e) => {
                    let paths = e.get_mut();
                    paths.push(rc_path.clone())
                }
                Entry::Vacant(e) => {
                    e.insert(vec![rc_path.clone()]);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use ethers::types::Address;

    use crate::arbitrage::arb_paths::{ArbPaths, ArbPathsErrors};
    use crate::arbitrage::arbs::ArbPool;

    #[test]
    fn test_arb_paths() {
        let mut arb_path = ArbPaths::default();
        let mut path = Vec::new();
        let token_0 = Address::random();
        let token_1 = Address::random();

        let pool_1_addr = Address::random();
        let pool_2_addr = Address::random();
        let pool_1 = ArbPool {
            pool: pool_1_addr,
            token_in: token_0,
            token_out: token_1,
        };
        let pool_2 = ArbPool {
            pool: pool_2_addr,
            token_in: token_1,
            token_out: token_0,
        };

        path.push(pool_1);
        path.push(pool_2);

        arb_path.insert_path(path).unwrap();

        assert_eq!(arb_path.get_paths(&pool_1_addr).unwrap().len(), 1);
        assert_eq!(arb_path.get_paths(&pool_2_addr).unwrap().len(), 1);
        assert_eq!(arb_path.get_paths(&pool_1_addr).unwrap()[0][0], pool_1);
        assert_eq!(arb_path.get_paths(&pool_1_addr).unwrap()[0][1], pool_2);
    }

    #[test]
    fn test_arb_path_dup_pool() {
        let mut arb_path = ArbPaths::default();
        let mut path = Vec::new();
        let token_0 = Address::random();
        let token_1 = Address::random();

        let pool_1_addr = Address::random();
        let pool_1 = ArbPool {
            pool: pool_1_addr,
            token_in: token_0,
            token_out: token_1,
        };
        let pool_2 = ArbPool {
            pool: pool_1_addr,
            token_in: token_1,
            token_out: token_0,
        };

        path.push(pool_1);
        path.push(pool_2);

        match arb_path.insert_path(path) {
            Err(ArbPathsErrors::DuplicatedPools) => (),
            _ => panic!("Not good"),
        }
    }
}
