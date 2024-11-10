use crate::auth::authenticate_basic;
use crate::r#trait::{AppState, AudioCommand, AudioState, AudioStreamHandle, ControlMessage, VideoCommand};
use axum::extract::ws::{Message, WebSocket};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, SampleFormat};
use futures::{SinkExt, StreamExt};
use opencv::{core::{Mat, Vector}, imgcodecs, prelude::*, videoio};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

const BUFFER_SIZE: usize = 1024;

pub async fn handle_video_socket(socket: WebSocket, state: AppState) {
    println!("New video websocket connection established");
    let (mut sender, mut receiver) = socket.split();

    let video_state = state.video_state.clone();
    let mut broadcast_rx = video_state.broadcast_tx.subscribe();

    let (tx, mut rx) = mpsc::channel::<VideoCommand>(10);
    let tx = Arc::new(tx); // Wrap in Arc for sharing

    let client_id = uuid::Uuid::new_v4().to_string();
    let client_id_for_sender = client_id.clone();
    let mut _video_task: Option<JoinHandle<()>> = None;
    let is_authenticated = Arc::new(TokioMutex::new(false));
    let is_authenticated_sender = is_authenticated.clone();
    let is_viewing = Arc::new(TokioMutex::new(false));
    let is_viewing_sender = is_viewing.clone();

    // Handle sending messages to client
    let sender_task = tokio::spawn(async move {
        println!("Sender task started for client {}", client_id_for_sender);
        loop {
            tokio::select! {
                Some(cmd) = rx.recv() => {
                    let msg = match cmd {
                        VideoCommand::Frame(data) => Message::Binary(data),
                        VideoCommand::Error(err) => {
                            println!("Sending error to client {}: {}", client_id_for_sender, err);
                            Message::Text(err)
                        },
                    };

                    if sender.send(msg).await.is_err() {
                        println!("Failed to send message to client {}, breaking sender task", client_id_for_sender);
                        break;
                    }
                }
                Ok(cmd) = broadcast_rx.recv() => {
                    let is_auth = *is_authenticated_sender.lock().await;
                    let is_view = *is_viewing_sender.lock().await;
                    if !is_auth || !is_view {
                        continue;
                    }

                    let msg = match cmd {
                        VideoCommand::Frame(data) => Message::Binary(data),
                        VideoCommand::Error(err) => Message::Text(err),
                    };

                    if sender.send(msg).await.is_err() {
                        break;
                    }
                }
            }
        }
        println!("Sender task ended for client {}", client_id_for_sender);
    });

    let tx_for_handler = tx.clone(); // Clone for message handling loop
    println!("Starting message handling loop for client {}", client_id);
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                let mut is_auth = is_authenticated.lock().await;
                if !*is_auth {
                    println!("Attempting authentication for client {}", client_id);
                    if authenticate_basic(&text).is_ok() {
                        *is_auth = true;
                        video_state.authenticated_clients.lock().await.insert(client_id.clone());
                        println!("Authentication successful for client {}", client_id);
                        let _ = tx_for_handler.send(VideoCommand::Error("Authenticated".to_string())).await;
                    } else {
                        println!("Authentication failed for client {}", client_id);
                        let _ = tx_for_handler.send(VideoCommand::Error("Unauthorized".to_string())).await;
                        break;
                    }
                    continue;
                }
                drop(is_auth);

                if let Ok(control_msg) = serde_json::from_str::<ControlMessage>(&text) {
                    if control_msg.message_type == "control" {
                        match control_msg.action.as_str() {
                            "on" => {
                                println!("Received ON command with index {:?} from client {}", control_msg.index, client_id);
                                if let Some(index) = control_msg.index {
                                    let mut viewing_clients = video_state.viewing_clients.lock().await;

                                    // Add client to viewing list
                                    viewing_clients.insert(client_id.clone());
                                    *is_viewing.lock().await = true;
                                    println!("Client {} added to viewing list. Total viewers: {}", client_id, viewing_clients.len());

                                    if viewing_clients.len() == 1 {
                                        // Create a new channel for the video task
                                        let (_video_tx, _video_rx) = mpsc::channel::<VideoCommand>(10);

                                        // Start video task if first viewer
                                        let state = state.clone();
                                        let broadcast_tx = video_state.broadcast_tx.clone();
                                        let tx_for_video = tx.clone(); // Clone for video task
                                        _video_task = Some(tokio::spawn(async move {
                                            println!("Video streaming task started");
                                            let mut interval = interval(Duration::from_millis(33));

                                            let io = match state.os_type.as_str() {
                                                "Linux" => videoio::CAP_V4L2,
                                                "Windows" => videoio::CAP_WINRT,
                                                "Darwin" => videoio::CAP_AVFOUNDATION,
                                                _ => videoio::CAP_ANY,
                                            };

                                            let mut camera = match videoio::VideoCapture::new(index, io) {
                                                Ok(cap) => {
                                                    if cap.is_opened().unwrap_or(false) {
                                                        println!("Camera initialized successfully");
                                                        let mut camera_guard = state.eyes.eyes_io.lock().await;
                                                        *camera_guard = Some(cap);
                                                        *state.current_camera_index.lock().await = Some(index);
                                                        let _ = tx_for_video.send(VideoCommand::Error("Video stream started".to_string())).await;
                                                        camera_guard.take().unwrap()
                                                    } else {
                                                        println!("Failed to open camera");
                                                        let _ = tx_for_video.send(VideoCommand::Error("Failed to open camera".to_string())).await;
                                                        return;
                                                    }
                                                },
                                                Err(e) => {
                                                    println!("Error creating camera: {:?}", e);
                                                    let _ = tx_for_video.send(VideoCommand::Error("Failed to create camera".to_string())).await;
                                                    return;
                                                }
                                            };

                                            let mut frame = Mat::default();
                                            let mut buf = Vector::new();
                                            let mut encode_params = Vector::new();
                                            encode_params.push(imgcodecs::IMWRITE_JPEG_QUALITY);
                                            encode_params.push(75);

                                            let mut last_frame_time = std::time::Instant::now();

                                            loop {
                                                interval.tick().await;

                                                if last_frame_time.elapsed() < Duration::from_millis(30) {
                                                    continue;
                                                }

                                                match camera.read(&mut frame) {
                                                    Ok(true) => {
                                                        // Clear buffer before reuse
                                                        buf.clear();

                                                        if imgcodecs::imencode(".jpg", &frame, &mut buf, &encode_params).unwrap_or(false) {
                                                            if broadcast_tx.send(VideoCommand::Frame(buf.to_vec())).is_err() {
                                                                println!("Failed to broadcast frame");
                                                                break;
                                                            }
                                                            last_frame_time = std::time::Instant::now();
                                                        }
                                                    },
                                                    Ok(false) => {
                                                        println!("Failed to read frame");
                                                        tokio::time::sleep(Duration::from_millis(10)).await;
                                                    },
                                                    Err(e) => {
                                                        println!("Error reading frame: {:?}", e);
                                                        tokio::time::sleep(Duration::from_millis(10)).await;
                                                    },
                                                }

                                                let viewing_count = state.video_state.viewing_clients.lock().await.len();
                                                if viewing_count == 0 {
                                                    println!("No viewers remaining, stopping stream");
                                                    break;
                                                }
                                            }

                                            // Cleanup camera
                                            let _ = camera.release();
                                            let mut camera_guard = state.eyes.eyes_io.lock().await;
                                            *camera_guard = None;
                                            *state.current_camera_index.lock().await = None;
                                            println!("Video streaming task ended");
                                        }));
                                    } else {
                                        let _ = tx_for_handler.send(VideoCommand::Error(
                                            "Joined existing stream".to_string()
                                        )).await;
                                    }
                                }
                            }
                            "off" => {
                                println!("Received OFF command from client {}", client_id);
                                let mut viewing_clients = video_state.viewing_clients.lock().await;

                                // Remove client from viewing list
                                viewing_clients.remove(&client_id);
                                *is_viewing.lock().await = false;
                                println!("Client {} removed from viewing list. Remaining viewers: {}", client_id, viewing_clients.len());

                                if viewing_clients.is_empty() {
                                    *state.eyes.status.lock().await = false;
                                    let _ = tx_for_handler.send(VideoCommand::Error("Eyes turned off".to_string())).await;
                                } else {
                                    let _ = tx_for_handler.send(VideoCommand::Error(
                                        "Stopped viewing. Other clients are still viewing.".to_string()
                                    )).await;
                                }
                            }
                            _ => {
                                let _ = tx_for_handler.send(VideoCommand::Error("Invalid action".to_string())).await;
                            }
                        }
                    }
                }
            }
            Message::Close(_) => {
                println!("Received close message from client {}", client_id);
                break;
            }
            _ => continue,
        }
    }

    // Cleanup
    println!("Cleaning up websocket handler for client {}", client_id);

    // Remove from both authenticated and viewing clients
    video_state.authenticated_clients.lock().await.remove(&client_id);
    video_state.viewing_clients.lock().await.remove(&client_id);

    sender_task.abort();
    println!("Video websocket handler terminated for client {}", client_id);
}

pub async fn handle_audio_socket(socket: WebSocket) {
    println!("New audio WebSocket connection established");
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<AudioCommand>(4);
    let ws_tx = tx.clone();

    let audio_state = Arc::new(TokioMutex::new(AudioState::new()));
    let stream_handle = Arc::new(TokioMutex::new(None));
    let audio_buffer = Arc::new(Mutex::new(Vec::new()));

    let sender_handle = tokio::spawn(async move {
        println!("Starting WebSocket sender task");
        while let Some(cmd) = rx.recv().await {
            // match &cmd {
            //     AudioCommand::Data(data) => println!("Sending audio data: {} bytes", data.len()),
            //     AudioCommand::Text(text) => println!("Sending text message: {}", text),
            // }

            let msg = match cmd {
                AudioCommand::Data(data) => Message::Binary(data),
                AudioCommand::Text(text) => Message::Text(text),
            };

            if let Err(e) = ws_sender.send(msg).await {
                println!("Failed to send WebSocket message: {:?}", e);
                break;
            }
        }
    });

    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            Message::Text(text) => {
                println!("Received text message: {}", text);
                let mut state = audio_state.lock().await;
                if !state.is_authenticated {
                    if authenticate_basic(&text).is_ok() {
                        state.is_authenticated = true;
                        drop(state);
                        let _ = tx.send(AudioCommand::Text("Authenticated".to_string())).await;
                    } else {
                        drop(state);
                        let _ = tx.send(AudioCommand::Text("Unauthorized".to_string())).await;
                        break;
                    }
                } else {
                    match text.as_str() {
                        "start_audio" => {
                            println!("Starting audio capture...");
                            let mut handle = stream_handle.lock().await;
                            if handle.is_none() {
                                let ws_tx = ws_tx.clone();
                                let audio_buffer = Arc::clone(&audio_buffer);

                                // Create a crossbeam channel for audio data
                                let (audio_sender, audio_receiver) = crossbeam_channel::bounded(32);

                                // Spawn a Tokio task to forward audio data to WebSocket
                                let forward_task = {
                                    let ws_tx = ws_tx.clone();
                                    tokio::spawn(async move {
                                        while let Ok(data) = audio_receiver.recv() {
                                            if let Err(e) = ws_tx.send(AudioCommand::Data(data)).await {
                                                println!("Failed to send audio data: {:?}", e);
                                                break;
                                            }
                                        }
                                    })
                                };

                                match setup_audio_stream(audio_buffer, audio_sender) {
                                    Ok(new_handle) => {
                                        println!("Audio stream setup successful");
                                        *handle = Some(new_handle);
                                        let _ = tx.send(AudioCommand::Text("Audio started".to_string())).await;
                                    }
                                    Err(e) => {
                                        println!("Failed to setup audio stream: {}", e);
                                        let _ = tx.send(AudioCommand::Text(format!("Failed to start audio: {}", e))).await;
                                        forward_task.abort();
                                    }
                                }
                            }
                        }
                        "stop_audio" => {
                            let mut handle = stream_handle.lock().await;
                            if let Some(h) = handle.take() {
                                stop_audio_stream(h);
                                let _ = tx.send(AudioCommand::Text("Audio stopped".to_string())).await;
                            }
                        }
                        _ => (),
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let mut handle = stream_handle.lock().await;
    if let Some(h) = handle.take() {
        stop_audio_stream(h);
    }
    sender_handle.abort();
}

fn setup_audio_stream(
    audio_buffer: Arc<Mutex<Vec<u8>>>,
    audio_sender: crossbeam_channel::Sender<Vec<u8>>,
) -> Result<AudioStreamHandle, String>
{
    let host = cpal::default_host();
    let device = host.default_input_device()
        .ok_or_else(|| "No input device available".to_string())?;

    println!("Using input device: {}", device.name().unwrap_or_default());

    let config = device.default_input_config()
        .map_err(|e| format!("Default config error: {:?}", e))?;

    println!("Using default config: {:?}", config);

    let stop_signal = Arc::new(Mutex::new(false));
    let stop_signal_clone = stop_signal.clone();

    match config.sample_format() {
        SampleFormat::F32 => {
            let stream = device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    if *stop_signal_clone.lock().unwrap() {
                        return;
                    }

                    let audio_data: Vec<u8> = data.iter()
                        .flat_map(|&sample| sample.to_ne_bytes().to_vec())
                        .collect();

                    let mut buffer = audio_buffer.lock().unwrap();
                    buffer.extend(audio_data);

                    if buffer.len() >= BUFFER_SIZE {
                        let data_to_send = buffer.split_off(0);
                        let _ = audio_sender.try_send(data_to_send);
                    }
                },
                move |err| {
                    eprintln!("Error in audio stream: {:?}", err);
                },
                Some(Duration::from_millis(10000)),
            ).map_err(|e| format!("Failed to build input stream: {:?}", e))?;

            stream.play().map_err(|e| format!("Failed to play stream: {:?}", e))?;
            println!("Audio stream started successfully");

            Ok(AudioStreamHandle {
                stream: Arc::new(stream),
                stop_signal,
            })
        }
        format => Err(format!("Unsupported sample format: {:?}", format))
    }
}

fn stop_audio_stream(handle: AudioStreamHandle) {
    println!("Stopping audio stream");
    *handle.stop_signal.lock().unwrap() = true;
    handle.stream.pause().unwrap_or_else(|e| eprintln!("Error stopping stream: {:?}", e));
}