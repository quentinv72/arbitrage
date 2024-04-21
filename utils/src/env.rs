use std::env;
use std::sync::{Arc, BarrierWaitResult};

use ethers::prelude::Provider;
use ethers::providers::{Middleware, Ws};
use ethers::types::Address;
use ethers_flashbots::FlashbotsMiddleware;
use log::{info, warn};

use crate::env::Env::{Production, Staging};
use crate::utils::{FlashbotsProvider, Setup, StagingProvider, Utils};

pub enum Env {
    Production(Utils<FlashbotsProvider>),
    Staging(Utils<StagingProvider>),
}

impl Env {
    pub async fn get_env() -> Self {
        match env::var("ENV") {
            Ok(val) => {
                let lowercase = val.to_lowercase();
                if lowercase == "prod" || lowercase == "production" {
                    let utils: Utils<FlashbotsProvider> = Utils::<FlashbotsProvider>::setup().await;
                    info!("We are in production baby");
                    Production(utils)
                } else {
                    let utils = Utils::<StagingProvider>::setup().await;
                    Staging(utils)
                }
            }
            Err(e) => {
                warn!("Couldn't find ENV key, setting env to staging... \n {e}");
                let utils: Utils<StagingProvider> = Utils::<StagingProvider>::setup().await;
                Staging(utils)
            }
        }
    }
    pub fn is_staging(&self) -> bool {
        match self {
            Production(_) => false,
            Staging(_) => true,
        }
    }

    pub fn get_rpc_client<M: Middleware>(&self) -> Arc<M> {
        match self {
            Production(inner) => Arc::clone(&inner.rpc_client),
            Staging(inner) => Arc::clone(&inner.rpc_client)
        }
    }

    pub fn get_ws_client(&self) -> Arc<Provider<Ws>> {
        match self {
            Production(inner) => Arc::clone(&inner.ws_provider),
            Staging(inner) => Arc::clone(&inner.ws_provider)
        }
    }

    pub fn get_bundle_executor_address(&self) -> Address {
        match self {
            Production(inner) => inner.bundle_executor,
            Staging(inner) => inner.bundle_executor
        }
    }
}