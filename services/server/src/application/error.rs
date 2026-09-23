//! Safe application failures mapped by transport adapters.

use super::{AuthorizationError, StoreError};
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum ApplicationError {
    InvalidInput(String),
    NotFound,
    Conflict,
    IdempotencyConflict,
    Authorization(AuthorizationError),
    Unavailable,
    Internal,
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => formatter.write_str(message),
            Self::NotFound => formatter.write_str("resource was not found"),
            Self::Conflict => formatter.write_str("resource already exists"),
            Self::IdempotencyConflict => {
                formatter.write_str("request ID was reused with different inputs")
            }
            Self::Authorization(error) => error.fmt(formatter),
            Self::Unavailable => formatter.write_str("application dependency is unavailable"),
            Self::Internal => formatter.write_str("internal application error"),
        }
    }
}

impl Error for ApplicationError {}

impl From<AuthorizationError> for ApplicationError {
    fn from(value: AuthorizationError) -> Self {
        Self::Authorization(value)
    }
}

impl From<StoreError> for ApplicationError {
    fn from(value: StoreError) -> Self {
        match value {
            StoreError::NotFound => Self::NotFound,
            StoreError::Conflict | StoreError::StaleRevision => Self::Conflict,
            StoreError::IdempotencyConflict => Self::IdempotencyConflict,
            StoreError::Unavailable => Self::Unavailable,
            StoreError::Internal => Self::Internal,
        }
    }
}
