use std::env;
use std::sync::Arc;

use ethers::addressbook::Address;
use ethers::core::k256::ecdsa::SigningKey;
use ethers::middleware::{Middleware, SignerMiddleware};
use ethers::prelude::{Http, LocalWallet, Provider, Wallet, Ws};
use ethers::signers::Signer;
use ethers_flashbots::BroadcasterMiddleware;
use log::{info, warn};
use url::Url;

use crate::utils::Env::{Production, Staging};

pub type FlashbotsProvider =
    SignerMiddleware<BroadcasterMiddleware<Provider<Http>, Wallet<SigningKey>>, Wallet<SigningKey>>;

static BUILDER_URLS: &[&str] = &[
    "https://rpc.beaverbuild.org",
    "https://relay.flashbots.net",
    "https://rsync-builder.xyz",
    "https://rpc.titanbuilder.xyz",
];

pub struct Utils<M: Middleware> {
    pub ws_client: Arc<Provider<Ws>>,
    pub rpc_client: Arc<M>,
    pub bundle_executor: Address,
    pub sender: Address,
    env: Env,
}

pub trait Setup<M: Middleware> {
    #[allow(async_fn_in_trait)]
    async fn setup() -> Utils<M>;
}

impl Setup<FlashbotsProvider> for Utils<FlashbotsProvider> {
    async fn setup() -> Utils<FlashbotsProvider> {
        let vars = Vars::init();
        let tx_signing_wallet: LocalWallet = vars.sender_private_key.unwrap().parse().unwrap();
        let tx_signing_wallet = tx_signing_wallet.with_chain_id(1_u32);
        let bundle_signing_wallet: LocalWallet = vars
            .flashbots_bundle_signing_private_key
            .unwrap()
            .parse()
            .unwrap();
        let provider = Provider::<Http>::try_from(vars.rpc_url.unwrap()).unwrap();
        let client = SignerMiddleware::new(
            BroadcasterMiddleware::new(
                provider,
                BUILDER_URLS
                    .iter()
                    .map(|url| Url::parse(url).unwrap())
                    .collect(),
                Url::parse("https://relay.flashbots.net").unwrap(),
                bundle_signing_wallet,
            ),
            tx_signing_wallet,
        );
        let client = Arc::new(client);
        let ws_client = Arc::new(Provider::<Ws>::connect(vars.ws_url.unwrap()).await.unwrap());
        Self {
            env: Env::get_env(),
            ws_client,
            rpc_client: client,
            bundle_executor: vars.bundle_executor_address.unwrap().parse().unwrap(),
            sender: vars.sender_address.unwrap(),
        }
    }
}

impl<M: Middleware> Utils<M> {
    pub fn is_production(&self) -> bool {
        match self.env {
            Production => true,
            Staging => false,
        }
    }

    pub fn is_staging(&self) -> bool {
        !self.is_production()
    }
}

struct Vars {
    rpc_url: Option<String>,
    ws_url: Option<String>,
    sender_private_key: Option<String>,
    sender_address: Option<Address>,
    flashbots_bundle_signing_private_key: Option<String>,
    bundle_executor_address: Option<String>,
}

impl Vars {
    fn init() -> Self {
        Vars {
            rpc_url: std::env::var("RPC_URL").ok(),
            ws_url: std::env::var("WS_URL").ok(),
            sender_private_key: std::env::var("SENDER_PRIVATE_KEY").ok(),
            sender_address: std::env::var("SENDER_ADDRESS")
                .ok()
                .map(|x| x.parse().unwrap()),
            flashbots_bundle_signing_private_key: std::env::var("BUNDLE_PRIVATE_KEY").ok(),
            bundle_executor_address: std::env::var("BUNDLE_EXECUTOR").ok(),
        }
    }
}

enum Env {
    Production,
    Staging,
}

impl Env {
    fn get_env() -> Self {
        match env::var("ENV") {
            Ok(val) => {
                let lowercase = val.to_lowercase();
                if lowercase == "prod" || lowercase == "production" {
                    info!("We are in production baby");
                    Production
                } else {
                    Staging
                }
            }
            Err(e) => {
                warn!("Couldn't find ENV key, setting env to staging... \n {e}");
                Staging
            }
        }
    }
}
