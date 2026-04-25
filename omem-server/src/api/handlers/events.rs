use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use tokio_stream::StreamExt;

use crate::api::server::AppState;

pub async fn sse_events(
    State(state): State<std::sync::Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let tenant_id = params.get("tenant_id").cloned().unwrap_or_default();
    let receiver = state.event_bus.subscribe();

    let stream = tokio_stream::wrappers::BroadcastStream::new(receiver).filter_map(move |result| {
        match result {
            Ok(event) => {
                let matches = tenant_id.is_empty()
                    || event.tenant_id.is_empty()
                    || event.tenant_id == tenant_id;
                if matches {
                    serde_json::to_string(&event)
                        .ok()
                        .map(|json| Ok(Event::default().data(json)))
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
