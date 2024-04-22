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

type FlashbotsProvider =
    SignerMiddleware<BroadcasterMiddleware<Provider<Http>, Wallet<SigningKey>>, Wallet<SigningKey>>;
static BUILDER_URLS: &[&str] = &[];

pub struct Utils<M: Middleware> {
    ws_client: Arc<Provider<Ws>>,
    rpc_client: Arc<M>,
    bundle_executor: Address,
    env: Env,
}

pub trait Setup<M: Middleware> {
    async fn setup() -> Utils<M>;
}

impl Setup<FlashbotsProvider> for Utils<FlashbotsProvider> {
    async fn setup() -> Self {
        let vars = Vars::init();
        let tx_signing_wallet: LocalWallet = vars.tx_signing_private_key.unwrap().parse().unwrap();
        let tx_signing_wallet = tx_signing_wallet.with_chain_id(1_u32);
        let bundle_signing_wallet: LocalWallet =
            vars.bundle_signing_private_key.unwrap().parse().unwrap();
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
            ws_client: ws_client,
            rpc_client: client,
            bundle_executor: vars.bundle_executor_address.unwrap().parse().unwrap(),
        }
    }
}

impl<M: Middleware> Utils<M> {
    pub fn get_rpc_client(&self) -> Arc<M> {
        Arc::clone(&self.rpc_client)
    }

    pub fn get_ws_client(&self) -> Arc<Provider<Ws>> {
        Arc::clone(&self.ws_client)
    }

    pub fn get_bundle_executor_address(&self) -> Address {
        self.bundle_executor
    }

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
    tx_signing_private_key: Option<String>,
    bundle_signing_private_key: Option<String>,
    bundle_executor_address: Option<String>,
}

impl Vars {
    fn init() -> Self {
        Vars {
            rpc_url: std::env::var("RPC_URL").ok(),
            ws_url: std::env::var("WS_URL").ok(),
            tx_signing_private_key: std::env::var("TX_PRIVATE_KEY").ok(),
            bundle_signing_private_key: std::env::var("BUNDLE_PRIVATE_KEY").ok(),
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
