use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

/// Logging middleware: generates request_id, creates tracing span, logs duration.
pub async fn logging_middleware(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        path = %path,
    );

    let start = std::time::Instant::now();
    tracing::info!(parent: &span, "request started");

    let response = next.run(request).await;

    let duration_ms = start.elapsed().as_millis();
    let status = response.status().as_u16();

    tracing::info!(
        parent: &span,
        status = status,
        duration_ms = duration_ms,
        "request completed"
    );

    response
}
