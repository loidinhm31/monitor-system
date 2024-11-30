use crate::r#trait::{AppState, CameraStatus, EyeInfo, SystemInfo};
use axum::extract::State;
use axum::Json;
use opencv::videoio;
use opencv::videoio::{VideoCaptureTrait, VideoCaptureTraitConst};
use std::collections::HashSet;

pub async fn get_system_info(
    State(state): State<AppState>,
) -> Json<SystemInfo> {
    let io = match state.os_type.as_str() {
        "Linux" => videoio::CAP_V4L2,
        "Windows" => videoio::CAP_WINRT,
        "Darwin" => videoio::CAP_AVFOUNDATION,
        _ => videoio::CAP_ANY,
    };

    let mut cameras = vec![];
    let current_camera = state.current_camera_index.lock().await.clone();
    let mut checked_ports = HashSet::new();

    // First, add the currently used camera if any
    if let Some(index) = current_camera {
        cameras.push(EyeInfo {
            index,
            name: format!("Camera {} (in use)", index),
            status: CameraStatus::InUse,
        });
        checked_ports.insert(index);
    }

    // Then check other available cameras
    for i in 0..10 {
        if checked_ports.contains(&i) {
            continue;
        }

        // Create a temporary capture to check availability
        match videoio::VideoCapture::new(i, io) {
            Ok(mut cap) => {
                if cap.is_opened().unwrap_or(false) {
                    cameras.push(EyeInfo {
                        index: i,
                        name: format!("Camera {}", i),
                        status: CameraStatus::Available,
                    });
                    // Make sure to release the capture immediately
                    let _ = cap.release();
                }
            },
            Err(_) => {
                // Skip unavailable cameras
                continue;
            }
        }
    }

    // Sort cameras by index for consistent ordering
    cameras.sort_by_key(|c| c.index);

    Json(SystemInfo {
        os_type: state.os_type.clone(),
        os_release: sys_info::os_release().unwrap_or_else(|_| "Unknown".to_string()),
        eyes: cameras,
    })
}
