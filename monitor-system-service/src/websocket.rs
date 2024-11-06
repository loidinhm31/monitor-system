use actix_web::web;
use cpal::Stream;
use std::sync::{Arc, Mutex};

use crate::AppState;

pub struct WebSocketSession {
    pub state: web::Data<AppState>,
    pub authenticated: bool,
}

pub struct AudioWebSocketSession {
    pub authenticated: bool,
    pub audio_stream: Option<Stream>,
    pub audio_streaming: bool,
    pub audio_buffer: Arc<Mutex<Vec<u8>>>,
}


