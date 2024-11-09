use std::collections::HashSet;
use opencv::videoio;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use cpal::Stream;
use tokio::sync::{broadcast, Mutex as TokioMutex};

#[derive(Clone)]
pub struct AppState {
    pub(crate) eyes: Arc<EyesState>,
    pub(crate) current_camera_index: Arc<TokioMutex<Option<i32>>>,
    pub(crate) os_type: String,
    pub(crate) video_state: Arc<VideoState>,
}

#[derive(Serialize, Deserialize)]
pub struct EyeInfo {
    pub index: i32,
    pub name: String,
}

pub struct EyesState {
    pub eyes_io: Arc<TokioMutex<Option<videoio::VideoCapture>>>,
    pub status: Arc<TokioMutex<bool>>,
}


#[derive(Serialize, Deserialize)]
pub struct SystemInfo {
    pub os_type: String,
    pub os_release: String,
    pub eyes: Vec<EyeInfo>,
}

#[derive(Deserialize)]
pub struct EyeRequest {
    pub action: String,
    pub index: Option<i32>,
}

#[derive(Debug, Clone)]
pub enum VideoCommand {
    Frame(Vec<u8>),
    Error(String),
}

#[derive(Debug, Deserialize)]
pub struct ControlMessage {
    #[serde(rename = "type")]
    pub(crate) message_type: String,
    pub(crate) action: String,
    pub(crate) index: Option<i32>,
}


#[derive(Debug)]
pub enum AudioCommand {
    Data(Vec<u8>),
    Text(String),
}

pub struct AudioStreamHandle {
    pub(crate) stream: Arc<Stream>,
    pub(crate) stop_signal: Arc<Mutex<bool>>,
}

unsafe impl Send for AudioStreamHandle {}



pub struct VideoState {
    pub(crate) broadcast_tx: broadcast::Sender<VideoCommand>,
    pub(crate) authenticated_clients: Arc<TokioMutex<HashSet<String>>>,
    pub(crate) viewing_clients: Arc<TokioMutex<HashSet<String>>>,
}

impl VideoState {
    pub fn new() -> Self {
        let (broadcast_tx, _) = broadcast::channel(10);
        Self {
            broadcast_tx,
            authenticated_clients: Arc::new(TokioMutex::new(HashSet::new())),
            viewing_clients: Arc::new(TokioMutex::new(HashSet::new())),
        }
    }
}


pub struct AudioState {
    pub(crate) is_authenticated: bool,
}

impl AudioState {
    pub fn new() -> Self {
        Self {
            is_authenticated: false,
        }
    }
}