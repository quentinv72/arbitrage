use std::sync::Arc;

use ethers::addressbook::Address;
use ethers::core::k256::ecdsa::SigningKey;
use ethers::middleware::{Middleware, SignerMiddleware};
use ethers::prelude::{Http, LocalWallet, Provider, Wallet, Ws};
use ethers::signers::Signer;
use ethers_flashbots::BroadcasterMiddleware;
use url::Url;

pub(crate) type StagingProvider = SignerMiddleware<Provider<Http>, Wallet<SigningKey>>;
pub(crate) type FlashbotsProvider = SignerMiddleware<BroadcasterMiddleware<Provider<Http>, Wallet<SigningKey>>, Wallet<SigningKey>>;


pub(crate) struct Utils<M: Middleware> {
    pub(crate) ws_provider: Arc<Provider<Ws>>,
    pub(crate) rpc_client: Arc<M>,
    pub(crate) bundle_executor: Address,
}

pub(crate) trait Setup<M: Middleware> {
    async fn setup() -> Utils<M>;
}

impl Setup<StagingProvider> for Utils<StagingProvider> {
    async fn setup() -> Self {
        let rpc_url = std::env::var("RPC_URL").unwrap();
        let ws_url = std::env::var("WS_URL").unwrap();
        let private_key = std::env::var("PRIVATE_KEY").unwrap();
        let bundle_executor_address = std::env::var("BUNDLE_EXECUTOR").unwrap();
        let wallet: LocalWallet = private_key.parse().unwrap();
        let wallet = wallet.with_chain_id(1_u32);
        let provider = Provider::<Http>::try_from(rpc_url).unwrap();
        let client = SignerMiddleware::new(provider, wallet);
        let client = Arc::new(client);
        let ws_client = Arc::new(Provider::<Ws>::connect(ws_url).await.unwrap());
        Self {
            ws_provider: ws_client,
            rpc_client: client,
            bundle_executor: bundle_executor_address.parse().unwrap(),
        }
    }
}

static BUILDER_URLS: &[&str] = &[];

impl Setup<FlashbotsProvider> for Utils<FlashbotsProvider> {
    async fn setup() -> Self {
        let rpc_url = std::env::var("RPC_URL").unwrap();
        let ws_url = std::env::var("WS_URL").unwrap();
        let private_key = std::env::var("PRIVATE_KEY").unwrap();
        let relay_signer = std::env::var("RELAY_SIGNER").unwrap();
        let bundle_executor_address = std::env::var("BUNDLE_EXECUTOR").unwrap();
        let wallet: LocalWallet = private_key.parse().unwrap();
        let wallet = wallet.with_chain_id(1_u32);
        let relay_wallet: LocalWallet = relay_signer.parse().unwrap();
        let provider = Provider::<Http>::try_from(rpc_url).unwrap();
        let client = SignerMiddleware::new(
            BroadcasterMiddleware::new(
                provider,
                BUILDER_URLS
                    .iter()
                    .map(|url| Url::parse(url).unwrap())
                    .collect(),
                Url::parse("https://relay.flashbots.net").unwrap(),
                relay_wallet,
            ),
            wallet,
        );
        let client = Arc::new(client);
        let ws_client = Arc::new(Provider::<Ws>::connect(ws_url).await.unwrap());
        Self {
            ws_provider: ws_client,
            rpc_client: client,
            bundle_executor: bundle_executor_address.parse().unwrap(),
        }
    }
}

