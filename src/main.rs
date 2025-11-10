use std::{sync::{Arc, Mutex}, time::Duration};
use axum::{Router, extract::Request, http::StatusCode, middleware::{self, Next}, response::IntoResponse, routing::get};
use chrono::{DateTime, Utc};
use tokio::{net::TcpListener, signal};
use uuid::Uuid;
struct Span {
    span_id: String,
    parent_id: Option<String>,
    name: String,
    start_time: DateTime<Utc>,
    end_time: Option<DateTime<Utc>>,
    duration_ms: Option<u128>,
}
#[derive(Clone)]
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

    fn start_span(&self, name: impl Into<String>, parent_id: Option<String>) -> String {
        let id = Uuid::new_v4().to_string();
        let span = Span {
            span_id: id.clone(),
            parent_id,
            name: name.into(),
            start_time: Utc::now(),
            end_time: None,
            duration_ms: None,
        };

        let mut g = self.spans.lock().unwrap();
        g.push(span);
        id
    }

    fn end_span(&self, span_id: &str) {
        let mut g = self.spans.lock().unwrap();
        if let Some(span) = g.iter_mut().rev().find(|s| s.span_id == span_id) {
            let end = Utc::now();
            span.end_time = Some(end);
            span.duration_ms = span
            .start_time
            .signed_duration_since(span.start_time)
            .to_std()
            .ok()
            .map(|_| end.signed_duration_since(span.start_time).num_milliseconds() as u128)
            .or_else(|| {
                Some((end.timestamp_millis() - span.start_time.timestamp_millis()) as u128)
            })
        }
    }
}

fn parse_traceparent(header_val: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = header_val.split("-").collect();
    if parts.len() >= 4 {
        let trace_id = parts[1].to_string();
        let parent = parts[2].to_string();
        Some((trace_id, parent))
    }else {
        None
    }
}

async fn trace_middleware(mut req: Request<axum::body::Body>, next: Next) -> impl IntoResponse {
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

    let root_name = format!(
        "{} {}",
        req.method(), 
        req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/"));

    let root_span_id = trace_ctx.start_span(root_name, incoming_parent_id);
    req.extensions_mut().insert(trace_ctx.clone());

    let response = next.run(req).await;

    trace_ctx.end_span(&root_span_id);
    let spans = trace_ctx.spans.lock().unwrap();
    println!("------------------------ Trace: {} -----------------------------", trace_ctx.trace_id);
        for span in spans.iter() {
            println!(
                "  Span: {} | Parent: {:?} | Name: {} | Duration: {:?}ms",
                span.span_id,
                span.parent_id,
                span.name,
                span.duration_ms
            );
        }
    println!("--------------------------------------------------------------\n");

    response
}

async fn get_user_handler(ctx_ext: Option<axum::extract::Extension<TraceContext>>) -> impl IntoResponse {
    let ctx = match ctx_ext {
        Some(axum::extract::Extension(c)) => c,
        None => {
            TraceContext::new()
        }
    };

    let route_span = ctx.start_span("Users route hit", None);
    let db_span = ctx.start_span("DB query execution", Some(route_span.clone()));

    ctx.end_span(&db_span);
    tokio::time::sleep(Duration::from_millis(80)).await;
    ctx.end_span(&route_span);
    (StatusCode::OK, "user data");
}   


#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/users/{id}", get(get_user_handler))
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