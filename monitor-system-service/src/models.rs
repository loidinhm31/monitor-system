use opencv::videoio;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

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