use std::fmt::Debug;

use async_trait::async_trait;
use ethers::middleware::{Middleware, MiddlewareError};
use ethers::prelude::MockProvider;
use thiserror::Error;

#[derive(Debug)]
pub struct PlaceholderMiddleware;

#[derive(Error, Debug)]
pub enum PlaceholderMiddlewareError {
    #[error("")]
    MiddlewareError,
}

impl MiddlewareError for PlaceholderMiddlewareError {
    type Inner = Self;

    fn from_err(_src: Self::Inner) -> PlaceholderMiddlewareError {
        unreachable!("")
    }

    fn as_inner(&self) -> Option<&Self::Inner> {
        unreachable!("")
    }
}

#[async_trait]
impl Middleware for PlaceholderMiddleware {
    type Error = PlaceholderMiddlewareError;
    type Provider = MockProvider;
    type Inner = Self;

    fn inner(&self) -> &Self::Inner {
        unreachable!("No inner provided here")
    }
}
