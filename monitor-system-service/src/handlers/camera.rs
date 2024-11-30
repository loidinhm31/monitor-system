use crate::processor::{
    camera_control::CameraServer,
    camera_control::CameraControl,
};
use crate::r#trait::AppState;
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, watch, RwLock};

pub type Users = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<Message>>>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WebRTCMessage {
    event: String,
    data: String,
    room: String,
    from: String,
    to: Option<String>,
}

pub async fn handle_video_socket(socket: WebSocket, app_state: AppState) {
    // Set up channels
    let (frame_tx, mut frame_rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = watch::channel(false);
    let (running_tx, running_rx) = watch::channel(true);
    let (camera_control_tx, mut camera_control_rx) = mpsc::channel(1);

    // Set up camera server with stop channel
    let camera = Arc::new(CameraServer::new(frame_tx, stop_rx, running_rx));

    // Clone camera for the control task
    let camera_for_control = camera.clone();
    let stop_tx = Arc::new(stop_tx);
    let running_tx = Arc::new(running_tx);

    // Clone app_state.user_sate for the frame broadcasting task
    let broadcast_state = app_state.user_sate.clone();

    // Spawn camera control task
    let camera_task = tokio::spawn({
        let stop_tx = stop_tx.clone();
        async move {
            while let Some(command) = camera_control_rx.recv().await {
                match command {
                    CameraControl::Start => {
                        let _ = stop_tx.send(false);
                        match camera_for_control.start_capture().await {
                            Ok(_) => println!("Camera capture ended normally"),
                            Err(e) => eprintln!("Camera error: {}", e),
                        }
                    }
                    CameraControl::Stop => {
                        let _ = stop_tx.send(true);
                        println!("Camera stop signal sent");
                    }
                }
            }
        }
    });

    // Spawn frame broadcasting task
    tokio::spawn(async move {
        while let Some(frame) = frame_rx.recv().await {
            let frame_msg = WebRTCMessage {
                event: "camera-frame".to_string(),
                data: frame,
                room: "default-room".to_string(),
                from: "server-camera".to_string(),
                to: None,
            };

            if let Ok(frame_str) = serde_json::to_string(&frame_msg) {
                broadcast_message(&broadcast_state, &frame_str, None).await;
            }
        }
    });

    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Task for sending messages to this client
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::from(msg)).await.is_err() {
                break;
            }
        }
    });

    // Clone app_state.user_sate again for the receive task
    let receive_state = app_state.user_sate.clone();

    // Clone camera_control_tx for use in the receive task
    let camera_tx = camera_control_tx.clone();

    // Task for receiving messages from this client
    let mut recv_task = tokio::spawn(async move {
        let mut user_id = String::new();
        let camera_control = Some(camera_tx);

        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                if let Ok(msg) = serde_json::from_str::<WebRTCMessage>(&text) {
                    println!("Received message: {:?}", msg.event);
                    match msg.event.as_str() {
                        "join" => {
                            user_id = msg.from.clone();
                            let user_joined_msg = serde_json::to_string(&WebRTCMessage {
                                event: "user_joined".to_string(),
                                data: user_id.clone(),
                                room: msg.room.clone(),
                                from: "server".to_string(),
                                to: None,
                            }).unwrap();

                            receive_state.write().await.insert(user_id.clone(), tx.clone());
                            broadcast_message(&receive_state, &user_joined_msg, Some(&user_id)).await;
                        }
                        "start-camera" => {
                            if let Some(tx) = &camera_control {
                                let _ = running_tx.send(true);
                                if let Err(e) = tx.send(CameraControl::Start).await {
                                    eprintln!("Failed to send camera start signal: {}", e);
                                }
                            }
                        }
                        "stop-camera" => {
                            if let Some(tx) = &camera_control {
                                if let Err(e) = tx.send(CameraControl::Stop).await {
                                    eprintln!("Failed to send camera stop signal: {}", e);
                                }
                                let _ = running_tx.send(false);
                            }
                        }
                        "message" => {
                            broadcast_message(&receive_state, &text, None).await;
                        }
                        "offer" | "answer" | "ice-candidate" => {
                            if let Some(to) = &msg.to {
                                if let Some(peer_tx) = receive_state.read().await.get(to) {
                                    let _ = peer_tx.send(Message::Text(text));
                                }
                            } else {
                                broadcast_message(&receive_state, &text, Some(&msg.from)).await;
                            }
                        }
                        _ => {
                            println!("Unknown message event: {}", msg.event);
                        }
                    }
                }
            }
        }

        // Cleanup when user disconnects
        if !user_id.is_empty() {
            if let Some(tx) = &camera_control {
                let _ = tx.send(CameraControl::Stop).await;
                let _ = running_tx.send(false);
            }

            receive_state.write().await.remove(&user_id);

            let user_left_msg = serde_json::to_string(&WebRTCMessage {
                event: "user_left".to_string(),
                data: user_id.clone(),
                room: "default-room".to_string(),
                from: "server".to_string(),
                to: None,
            }).unwrap();
            broadcast_message(&receive_state, &user_left_msg, None).await;
        }
    });

    tokio::select! {
        _ = camera_task => println!("Camera task completed"),
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
}

pub async fn broadcast_message(state: &Users, message: &str, exclude_user: Option<&str>) {
    let users = state.read().await;
    for (user_id, tx) in users.iter() {
        if exclude_user.map_or(true, |excluded| user_id != excluded) {
            let _ = tx.send(Message::Text(message.to_string()));
        }
    }
}