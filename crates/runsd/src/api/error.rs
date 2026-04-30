use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use common::model::ProblemDetail;

use crate::error::AppError;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, type_uri, title, detail) = match &self {
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "urn:runsd:error:not-found",
                "Not Found",
                None,
            ),
            AppError::Conflict(msg) => (
                StatusCode::CONFLICT,
                "urn:runsd:error:conflict",
                "Conflict",
                Some(msg.as_str()),
            ),
            AppError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                "urn:runsd:error:bad-request",
                "Bad Request",
                Some(msg.as_str()),
            ),
            AppError::ServiceUnavailable(msg) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "urn:runsd:error:service-unavailable",
                "Service Unavailable",
                Some(msg.as_str()),
            ),
            AppError::Cancelled => (
                StatusCode::OK,
                "urn:runsd:error:cancelled",
                "Cancelled",
                None,
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "urn:runsd:error:internal",
                "Internal Server Error",
                None,
            ),
        };

        let body = ProblemDetail {
            type_uri: type_uri.into(),
            title: title.into(),
            status: status.as_u16(),
            detail: detail.map(|s| s.to_string()),
        };

        let mut response = (status, Json(body)).into_response();
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}
