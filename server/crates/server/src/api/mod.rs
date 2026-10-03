use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::store::StoreError;
use wire::ApiError;

pub mod chat;
pub mod llm;
pub mod router;
pub mod stream;
pub mod wire;
pub mod world;

/// An `/api` error answer: the status with an [`ApiError`] body.
pub fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(ApiError { error: message.to_owned() })).into_response()
}

/// A failed handler; storage errors become a logged 503.
#[derive(Debug)]
pub struct ApiFailure(pub StatusCode, pub String);

impl From<StoreError> for ApiFailure {
    fn from(e: StoreError) -> Self {
        tracing::error!(error = %e, "storage failed");
        Self(StatusCode::SERVICE_UNAVAILABLE, "storage unavailable".into())
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        error(self.0, &self.1)
    }
}
