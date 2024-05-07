use std::collections::HashSet;

use ethers::middleware::Middleware;
use ethers::signers::Signer;
use ethers::types::Address;
use once_cell::sync::Lazy;

pub mod logging;
pub mod utils;

pub static TOKEN_BLACKLIST: Lazy<HashSet<Address>> = Lazy::new(|| {
    let mut set = HashSet::new();
    set.insert("0x9A2Af0AbB12bee5369B180976Be01E8c80D0e7B6".parse().unwrap());
    set.insert("0xb220D53F7D0f52897Bcf25E47c4c3DC0bac344F8".parse().unwrap());
    // Unclear why this token fails to transfer...
    set.insert("0xA4EED63db85311E22dF4473f87CcfC3DaDCFA3E3".parse().unwrap());
    set.insert("0x7FF4169a6B5122b664c51c95727d87750eC07c84".parse().unwrap());
    set.insert("0x8B3192f5eEBD8579568A2Ed41E6FEB402f93f73F".parse().unwrap());
    set.insert("0x6aA614B6FcDEC46f74cE86e74706ec4c815B3da0".parse().unwrap());
    // Need ERC223 tokenFallback method to be implemented on contract, but weirdly this contract implements
    // tokenFallback instead of tokenReceived
    set.insert("0xa4ec83c8907888d006A37debF755ee39766f38ae".parse().unwrap());
    set.insert("0x1Efb2286BF89F01488C6B2a22B2556C0f45e972b".parse().unwrap());
    set.insert("0x9C061DF134d11412151E9c200ce3F9f6F295094a".parse().unwrap());
    set.insert("0x378E8c47eb42cCE0dd9Cff48276a2aB73e9C254F".parse().unwrap());
    set.insert("0x6f3277ad0782a7DA3eb676b85a8346A100BF9C1c".parse().unwrap());
    set.insert("0x1B8568FbB47708E9E9D31Ff303254f748805bF21".parse().unwrap());
    set.insert("0x91a5de30e57831529a3c1aF636A78a7E4E83f3aa".parse().unwrap());
    set.insert("0x966f2262aB963514cB0d4ec5721Af209879a1Fd9".parse().unwrap());
    set
});
