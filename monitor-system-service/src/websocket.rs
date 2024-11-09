use crate::{auth::authenticate_basic, AppState};
use axum::extract::ws::{Message, WebSocket};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, SampleFormat, Stream};
use opencv::{core::{Mat, Vector}, imgcodecs, prelude::*, videoio};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch, Mutex as TokioMutex};
use tokio::time::{interval, Duration};


const BUFFER_SIZE: usize = 1024;

#[derive(Debug)]
enum VideoCommand {
    Frame(Vec<u8>),
    Error(String),
}

#[derive(Debug, Deserialize)]
struct ControlMessage {
    #[serde(rename = "type")]
    message_type: String,
    action: String,
    index: Option<i32>,
}


#[derive(Debug)]
enum AudioCommand {
    Data(Vec<u8>),
    Text(String),
}

struct AudioStreamHandle {
    stream: Arc<Stream>,
    stop_signal: Arc<Mutex<bool>>,
}

unsafe impl Send for AudioStreamHandle {}


// Audio state that can be shared between threads
struct AudioState {
    is_authenticated: bool,
}

impl AudioState {
    fn new() -> Self {
        Self {
            is_authenticated: false,
        }
    }
}

pub async fn handle_video_socket(socket: WebSocket, state: AppState) {
    println!("New video websocket connection established");
    let (mut sender, mut receiver) = socket.split();
    // Use a smaller channel size to prevent frame buildup
    let (tx, mut rx) = tokio::sync::mpsc::channel::<VideoCommand>(10);
    let (stream_tx, stream_rx) = watch::channel((false, None::<i32>));

    let mut is_authenticated = false;
    let mut video_task = None;

    // Optimize sender task to handle backpressure
    let sender_task = tokio::spawn(async move {
        println!("Sender task started");
        while let Some(cmd) = rx.recv().await {
            let msg = match cmd {
                VideoCommand::Frame(data) => Message::Binary(data),
                VideoCommand::Error(err) => {
                    println!("Sending error: {}", err);
                    Message::Text(err)
                },
            };

            if sender.send(msg).await.is_err() {
                println!("Failed to send message, breaking sender task");
                break;
            }
        }
        println!("Sender task ended");
    });

    println!("Starting message handling loop");
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if !is_authenticated {
                    println!("Attempting authentication");
                    if authenticate_basic(&text).is_ok() {
                        is_authenticated = true;
                        println!("Authentication successful");

                        let stream_tx_clone = stream_tx.clone();
                        video_task = Some({
                            let tx = tx.clone();
                            let state = state.clone();
                            let stream_rx = stream_rx.clone();

                            tokio::spawn(async move {
                                println!("Video streaming task started");
                                // Use a more precise interval for frame timing
                                let mut interval = interval(Duration::from_millis(33)); // 30 FPS
                                let mut stream_rx = stream_rx;

                                // Pre-allocate reusable buffers
                                let mut frame = Mat::default();
                                let mut buf = Vector::new();
                                let mut encode_params = Vector::new();
                                encode_params.push(imgcodecs::IMWRITE_JPEG_QUALITY);
                                encode_params.push(75); // Reduce quality for better performance

                                // Track frame timing
                                let mut last_frame_time = std::time::Instant::now();

                                loop {
                                    interval.tick().await;

                                    let (is_streaming, camera_index) = *stream_rx.borrow();
                                    if !is_streaming {
                                        if stream_rx.changed().await.is_err() {
                                            println!("Stream channel closed");
                                            break;
                                        }
                                        continue;
                                    }

                                    // Skip frame if we're behind
                                    if last_frame_time.elapsed() < Duration::from_millis(30) {
                                        continue;
                                    }

                                    let mut camera_guard = state.eyes.eyes_io.lock().await;
                                    if camera_guard.is_none() {
                                        if let Some(index) = camera_index {
                                            println!("Initializing camera with index: {}", index);
                                            let io = match state.os_type.as_str() {
                                                "Linux" => videoio::CAP_V4L2,
                                                "Windows" => videoio::CAP_WINRT,
                                                "Darwin" => videoio::CAP_AVFOUNDATION,
                                                _ => videoio::CAP_ANY,
                                            };

                                            match videoio::VideoCapture::new(index, io) {
                                                Ok(cap) => {
                                                    if cap.is_opened().unwrap_or(false) {
                                                        println!("Camera initialized successfully");
                                                        *camera_guard = Some(cap);
                                                        *state.current_camera_index.lock().await = Some(index);
                                                    } else {
                                                        println!("Failed to open camera");
                                                        let _ = tx.send(VideoCommand::Error("Failed to open camera".to_string())).await;
                                                        let _ = stream_tx_clone.send((false, None));
                                                        continue;
                                                    }
                                                },
                                                Err(e) => {
                                                    println!("Error creating camera: {:?}", e);
                                                    let _ = tx.send(VideoCommand::Error("Failed to create camera".to_string())).await;
                                                    let _ = stream_tx_clone.send((false, None));
                                                    continue;
                                                }
                                            }
                                        } else {
                                            let _ = tx.send(VideoCommand::Error("No camera index specified".to_string())).await;
                                            let _ = stream_tx_clone.send((false, None));
                                            continue;
                                        }
                                    }

                                    if let Some(ref mut camera) = *camera_guard {
                                        match camera.read(&mut frame) {
                                            Ok(true) => {
                                                // Clear buffer before reuse
                                                buf.clear();

                                                if imgcodecs::imencode(".jpg", &frame, &mut buf, &encode_params).unwrap_or(false) {
                                                    // Use try_send to implement backpressure
                                                    match tx.try_send(VideoCommand::Frame(buf.to_vec())) {
                                                        Ok(_) => {
                                                            last_frame_time = std::time::Instant::now();
                                                        },
                                                        Err(mpsc::error::TrySendError::Full(_)) => {
                                                            // Skip frame if channel is full
                                                            continue;
                                                        },
                                                        Err(_) => break,
                                                    }
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
                                    }

                                    let (is_streaming, _) = *stream_rx.borrow();
                                    if !is_streaming {
                                        println!("Streaming disabled, releasing camera");
                                        if let Some(ref mut camera) = *camera_guard {
                                            let _ = camera.release();
                                        }
                                        *camera_guard = None;
                                        *state.current_camera_index.lock().await = None;
                                    }
                                }
                                println!("Video streaming task ended");
                            })
                        });

                        let _ = tx.send(VideoCommand::Error("Authenticated".to_string())).await;
                    } else {
                        println!("Authentication failed");
                        let _ = tx.send(VideoCommand::Error("Unauthorized".to_string())).await;
                        break;
                    }
                    continue;
                }

                // Rest of the control message handling remains the same...
                if let Ok(control_msg) = serde_json::from_str::<ControlMessage>(&text) {
                    if control_msg.message_type == "control" {
                        match control_msg.action.as_str() {
                            "on" => {
                                println!("Received ON command with index {:?}", control_msg.index);
                                if let Some(index) = control_msg.index {
                                    let camera_guard = state.eyes.eyes_io.lock().await;
                                    let mut status_guard = state.eyes.status.lock().await;

                                    if *status_guard {
                                        if camera_guard.is_some() {
                                            let _ = tx.send(VideoCommand::Error(
                                                "Turn off the current eyes before selecting a new one".to_string()
                                            )).await;
                                            continue;
                                        }
                                    }

                                    if state.current_camera_index.lock().await.is_none() {
                                        let _ = stream_tx.send((true, Some(index)));
                                        *status_guard = true;
                                        let _ = tx.send(VideoCommand::Error(
                                            "Video stream started".to_string()
                                        )).await;
                                    } else {
                                        let _ = tx.send(VideoCommand::Error(
                                            "Eyes are already running".to_string()
                                        )).await;
                                    }
                                } else {
                                    let _ = tx.send(VideoCommand::Error(
                                        "Select the right eyes number".to_string()
                                    )).await;
                                }
                            }
                            "off" => {
                                println!("Received OFF command");
                                let mut camera_guard = state.eyes.eyes_io.lock().await;
                                *state.eyes.status.lock().await = false;

                                if camera_guard.is_some() {
                                    let _ = stream_tx.send((false, None));
                                    camera_guard.as_mut().unwrap().release().unwrap();
                                    *camera_guard = None;
                                    *state.current_camera_index.lock().await = None;
                                    let _ = tx.send(VideoCommand::Error(
                                        "Eyes turned off".to_string()
                                    )).await;
                                } else {
                                    let _ = tx.send(VideoCommand::Error(
                                        "Eyes are not turned off correctly".to_string()
                                    )).await;
                                }
                            }
                            _ => {
                                let _ = tx.send(VideoCommand::Error("Invalid action".to_string())).await;
                            }
                        }
                    }
                } else {
                    let _ = tx.send(VideoCommand::Error("Invalid message format".to_string())).await;
                }
            }
            Message::Close(_) => {
                println!("Received close message");
                break;
            }
            _ => continue,
        }
    }

    // Cleanup
    println!("Cleaning up websocket handler");
    let _ = stream_tx.send((false, None));
    if let Some(task) = video_task {
        task.abort();
    }
    sender_task.abort();

    let mut camera_guard = state.eyes.eyes_io.lock().await;
    if let Some(ref mut camera) = *camera_guard {
        let _ = camera.release();
    }
    *camera_guard = None;
    *state.current_camera_index.lock().await = None;

    println!("Video websocket handler terminated");
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
            _ => (),
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
                Some(Duration::from_millis(10000))
            ).map_err(|e| format!("Failed to build input stream: {:?}", e))?;

            stream.play().map_err(|e| format!("Failed to play stream: {:?}", e))?;
            println!("Audio stream started successfully");

            Ok(AudioStreamHandle {
                stream: Arc::new(stream),
                stop_signal,
            })
        },
        format => Err(format!("Unsupported sample format: {:?}", format))
    }
}

fn stop_audio_stream(handle: AudioStreamHandle) {
    println!("Stopping audio stream");
    *handle.stop_signal.lock().unwrap() = true;
    handle.stream.pause().unwrap_or_else(|e| eprintln!("Error stopping stream: {:?}", e));
}