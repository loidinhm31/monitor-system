use std::sync::{Arc, Mutex};
use actix_web::web;
use cpal::Stream;
use cpal::traits::DeviceTrait;

use crate::AppState;

pub struct WebSocketSession {
    pub state: web::Data<AppState>,
    pub authenticated: bool,
}

pub struct AudioWebSocketSession {
    pub authenticated: bool,
    pub audio_stream: Option<Stream>,
    pub audio_streaming: bool,
}


