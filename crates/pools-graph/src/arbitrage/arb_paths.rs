use std::collections::HashMap;
use std::rc::Rc;

use ethers::types::Address;

use crate::arbitrage::arbs::ArbPool;

#[derive(Default)]
struct ArbPaths {
    // Maps a pool identifier to a list of arbitrage paths. The paths are wrapped in an Rc<_> so
    // that we don't need to clone each Vec multiple times.
    pool_to_paths: HashMap<Address, Vec<Rc<Vec<ArbPool>>>>,
}

impl ArbPaths {
    #[inline]
    pub fn get_paths(&self, addr: &Address) -> Option<&Vec<Rc<Vec<ArbPool>>>> {
        self.pool_to_paths.get(addr)
    }

    #[inline]
    pub fn insert_path(&mut self, path: Vec<ArbPool>) {
        let rc_path = Rc::new(path);
        for arb_pool in rc_path.iter() {
            let key = arb_pool.pool;
            if self.pool_to_paths.contains_key(&key) {
                self.pool_to_paths.get_mut(&key).unwrap().push(rc_path.clone());
            } else {
                let mut paths = Vec::new();
                paths.push(rc_path.clone());
                self.pool_to_paths.insert(key, paths);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ethers::types::Address;

    use crate::arbitrage::arb_paths::ArbPaths;
    use crate::arbitrage::arbs::ArbPool;

    #[test]
    fn arb_paths() {
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

        arb_path.insert_path(path);

        assert_eq!(arb_path.get_paths(&pool_1_addr).unwrap().len(), 1);
        assert_eq!(arb_path.get_paths(&pool_2_addr).unwrap().len(), 1);
        assert_eq!(arb_path.get_paths(&pool_1_addr).unwrap()[0][0], pool_1);
        assert_eq!(arb_path.get_paths(&pool_1_addr).unwrap()[0][1], pool_2);
    }
}