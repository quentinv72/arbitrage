use enum_dispatch::enum_dispatch;
use ethers::types::{Address, U256};

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
#[enum_dispatch(GetAddress)]
pub enum Factory {
    UniswapV3Factoru(FactoryV3),
    UniswapV2Factory(FactoryV2),
}

#[enum_dispatch]
pub trait GetAddress {
    fn get_address(&self) -> Address;
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct FactoryV2 {
    pub address: Address,
    pub swap_fee: U256,
}

impl GetAddress for FactoryV2 {
    fn get_address(&self) -> Address {
        self.address
    }
}

impl Default for FactoryV2 {
    fn default() -> Self {
        FactoryV2 {
            address: Address::zero(),
            swap_fee: U256::from(3),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub struct FactoryV3 {
    pub address: Address,
    pub subgraph_url: &'static str,
    pub quoter_address: Address,
}

impl GetAddress for FactoryV3 {
    fn get_address(&self) -> Address {
        self.address
    }
}

impl Default for FactoryV3 {
    fn default() -> Self {
        FactoryV3 {
            address: "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            subgraph_url: "",
            quoter_address: "0x0000000000000000000000000000000000000002"
                .parse()
                .unwrap(),
        }
    }
}
