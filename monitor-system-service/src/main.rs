use crate::handlers::camera::{handle_video_socket, Users};
use crate::handlers::system_info::get_system_info;
use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get
    , Router,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod r#trait;
mod websocket;
mod handlers;
mod processor;

use crate::r#trait::{AppState, EyesState, VideoState};
use crate::websocket::handle_audio_socket;


async fn healthcheck() -> &'static str {
    "Up"
}

async fn video_websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_video_socket(socket, state))
}

async fn audio_websocket_handler(
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(handle_audio_socket)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let users: Users = Arc::new(RwLock::new(HashMap::new()));

    let os_type = sys_info::os_type().unwrap();
    let eyes = EyesState {
        eyes_io: Arc::new(TokioMutex::new(None)),
        status: Arc::new(TokioMutex::new(false)),
    };

    let state = AppState {
        eyes: Arc::new(eyes),
        current_camera_index: Arc::new(TokioMutex::new(None)),
        os_type,
        video_state: Arc::new(VideoState::new()),
        user_sate: users.clone()
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/healthz", get(healthcheck))
        .route("/ws", get(video_websocket_handler))
        .route("/sensors/ears/ws", get(audio_websocket_handler))
        .route("/system", get(get_system_info))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await.unwrap();
    println!("Server running at http://0.0.0.0:8081");
    axum::serve(listener, app).await.unwrap();
}

