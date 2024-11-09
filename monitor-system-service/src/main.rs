use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use opencv::{prelude::*, videoio};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod r#trait;
mod websocket;

use crate::r#trait::{AppState, EyeInfo, EyesState, SystemInfo, VideoState};
use crate::websocket::{handle_audio_socket, handle_video_socket};


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


async fn get_system_info(
    State(state): State<AppState>,
) -> Json<SystemInfo> {
    let io = match state.os_type.as_str() {
        "Linux" => videoio::CAP_V4L2,
        "Windows" => videoio::CAP_WINRT,
        "Darwin" => videoio::CAP_AVFOUNDATION,
        _ => videoio::CAP_ANY,
    };

    let mut cameras = vec![];
    for i in 0..10 {
        if let Ok(mut cap) = videoio::VideoCapture::new(i, io) {
            if cap.is_opened().unwrap() {
                cameras.push(EyeInfo {
                    index: i,
                    name: format!("Camera {}", i),
                });
                cap.release().unwrap();
            }
        }
    }

    Json(SystemInfo {
        os_type: state.os_type.clone(),
        os_release: sys_info::os_release().unwrap_or_else(|_| "Unknown".to_string()),
        eyes: cameras,
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

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
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/healthz", get(healthcheck))
        .route("/sensors/eyes/ws", get(video_websocket_handler))
        .route("/sensors/ears/ws", get(audio_websocket_handler))
        .route("/system", get(get_system_info))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await.unwrap();
    println!("Server running at http://0.0.0.0:8081");
    axum::serve(listener, app).await.unwrap();
}