use std::sync::{Arc, Mutex};
use axum::{Router, extract::Request, middleware::{self, Next}, response::IntoResponse, routing::get};
use chrono::{DateTime, Utc};
use tokio::{net::TcpListener, signal};
use uuid::Uuid;
struct Span {
    span_id: String,
    parent_id: Option<String>,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    duration_ms: Option<u128>,
}

struct TraceContext {
    trace_id: String,
    spans: Arc<Mutex<Vec<Span>>>
}

impl TraceContext {
    fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string(),
            spans: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

async fn trace_middleware<B>(mut req: Request<B>, next: Next) -> impl IntoResponse {
    let headers = req.headers();
    let (trace_ctx, incoming_parent_id) = if let Some(tp) = headers.get("traceparent") {
        if let Ok(s) = tp.to_str() {
            if let Some((trace_id, parent)) = parse_traceparent(s) {
                let ctx = TraceContext {
                    trace_id: trace_id,
                    spans: Arc::new(Mutex::new(Vec::new()))
                };
                (ctx, Some(parent))
            } else {
                (TraceContext::new(), None)
            }
        } else {
            (TraceContext::new(), None)
        }
    } else {
         (TraceContext::new(), None) 
    };
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(hello))
        .layer(middleware::from_fn(trace_middleware));
    
    let address = "127.0.0.1:3000";
    let listener = TcpListener::bind(&address).await.unwrap();

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = signal::ctrl_c().await;
        })
        .await
        .unwrap()
}