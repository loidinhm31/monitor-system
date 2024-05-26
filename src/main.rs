use std::sync::{Arc, Mutex};
use actix::{Actor, StreamHandler, AsyncContext, Message, Handler, ActorContext};
use actix_cors::Cors;
use actix_web::{App, HttpServer, HttpResponse, web, Error, HttpRequest};
use actix_web::web::Payload;
use actix_web_actors::ws;
use base64::{Engine as _, engine::general_purpose};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, Stream};
use opencv::prelude::*;
use opencv::videoio;
use serde::{Deserialize, Serialize};
use sys_info;
use opencv::core::Vector;
use opencv::imgcodecs;

#[derive(Serialize, Deserialize)]
struct EyeInfo {
    index: i32,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct SystemInfo {
    os_type: String,
    os_release: String,
    eyes: Vec<EyeInfo>,
}

#[derive(Deserialize)]
struct EyeRequest {
    action: String,
    index: Option<i32>,
}

struct AppState {
    eyes: EyesState,
    current_camera_index: Arc<Mutex<Option<i32>>>,
    os_type: String,
}

struct EyesState {
    eyes_io: Arc<Mutex<Option<videoio::VideoCapture>>>,
    status: Arc<Mutex<bool>>,
}

struct WebSocketSession {
    state: web::Data<AppState>,
    authenticated: bool,
    audio_stream: Option<Stream>,
}

struct AudioData(Vec<u8>);

impl Message for AudioData {
    type Result = ();
}

impl Handler<AudioData> for WebSocketSession {
    type Result = ();

    fn handle(&mut self, msg: AudioData, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }
}

impl Actor for WebSocketSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let state = self.state.clone();
        ctx.run_interval(std::time::Duration::from_millis(100), move |act, ctx| {
            if act.authenticated {
                let mut camera_guard = state.eyes.eyes_io.lock().unwrap();
                if let Some(ref mut camera) = *camera_guard {
                    let mut frame = Mat::default();
                    if camera.read(&mut frame).is_ok() {
                        let mut buf = Vector::new();
                        if imgcodecs::imencode(".jpg", &frame, &mut buf, &Vector::new()).is_ok() {
                            ctx.binary(buf.to_vec());
                        }
                    }
                }
            }
        });

        let host = cpal::default_host();
        let audio_input_device = host.default_input_device().expect("No input device available");
        let audio_input_config = audio_input_device.default_input_config().unwrap();

        match audio_input_config.sample_format() {
            SampleFormat::F32 => {
                let addr = ctx.address();
                let stream = audio_input_device.build_input_stream(
                    &audio_input_config.into(),
                    move |data: &[f32], _| {
                        let audio_data = data.iter().map(|&sample| sample.to_ne_bytes().to_vec()).flatten().collect::<Vec<_>>();
                        addr.do_send(AudioData(audio_data));
                    },
                    |err| {
                        eprintln!("Error occurred on audio input stream: {:?}", err);
                    }
                ).unwrap();
                stream.play().unwrap();
                self.audio_stream = Some(stream);
            },
            _ => panic!("Unsupported sample format"),
        }
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebSocketSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        println!("Message: {:?}", msg);
        match msg {
            Ok(ws::Message::Text(text)) => {
                if !self.authenticated {
                    if authenticate_basic(&text).is_ok() {
                        self.authenticated = true;
                        ctx.text("Authenticated");
                    } else {
                        ctx.text("Unauthorized");
                        ctx.stop();
                    }
                }
            }
            Ok(ws::Message::Binary(bin)) => {
                // Handle binary messages if necessary
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => (),
        }
    }
}

async fn sensors_eyes_event_web_socket(r: HttpRequest, stream: Payload, data: web::Data<AppState>) -> Result<HttpResponse, Error> {
    ws::start(WebSocketSession { state: data, authenticated: false, audio_stream: None }, &r, stream)
}

async fn turn_eyes_on_off(request: web::Json<EyeRequest>, data: web::Data<AppState>) -> HttpResponse {
    return if request.action == "on" {
        let mut eyes_guard = data.eyes.eyes_io.lock().unwrap();

        if *data.eyes.status.lock().unwrap() == true {
            if eyes_guard.is_some() {
                return HttpResponse::BadRequest().body("Turn off the current eyes before selecting a new one");
            }
            HttpResponse::BadRequest().body("Not valid status and state of eyes")
        } else {
            if eyes_guard.is_none() {
                if request.index.is_some() {
                    let camera_index = request.index.unwrap();

                    if data.current_camera_index.lock().unwrap().is_none() {
                        let io = match data.os_type.as_str() {
                            "Linux" => videoio::CAP_V4L2,
                            "Windows" => videoio::CAP_WINRT,
                            "Darwin" => videoio::CAP_AVFOUNDATION,
                            _ => videoio::CAP_ANY,
                        };

                        *eyes_guard = Some(videoio::VideoCapture::new(camera_index, io).unwrap());
                        if !eyes_guard.as_ref().unwrap().is_opened().unwrap() {
                            *eyes_guard = None;
                            return HttpResponse::InternalServerError().body("Unable to open eyes");
                        } else {
                            *data.eyes.status.lock().unwrap() = true;
                        }
                        return HttpResponse::Ok().body("Eyes turned on");
                    } else {
                        println!("Running eyes already selected. You need to stop the current one.");
                        HttpResponse::BadRequest().body("Eyes are already running")
                    }
                } else {
                    HttpResponse::BadRequest().body("Select the right eyes number")
                }
            } else {
                HttpResponse::InternalServerError().body("Eyes are not turned on")
            }
        }
    } else if request.action == "off" {
        let mut eyes_guard = data.eyes.eyes_io.lock().unwrap();
        *data.eyes.status.lock().unwrap() = false;

        if eyes_guard.is_some() {
            eyes_guard.as_mut().unwrap().release().unwrap();
            *eyes_guard = None;
            return HttpResponse::Ok().body("Eyes turned off");
        }
        return HttpResponse::InternalServerError().body("Eyes are not turned off correctly");
    } else {
        HttpResponse::BadRequest().body("Invalid action")
    };
}

async fn get_system_info(data: web::Data<AppState>) -> HttpResponse {
    let io = match data.os_type.as_str() {
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

    let system_info = SystemInfo {
        os_type: data.os_type.clone(),
        os_release: sys_info::os_release().unwrap_or_else(|_| "Unknown".to_string()),
        eyes: cameras,
    };

    HttpResponse::Ok().json(system_info)
}

fn authenticate_basic(auth_str: &str) -> Result<(), HttpResponse> {
    if auth_str.starts_with("Basic ") {
        let encoded = &auth_str[6..];
        if let Ok(decoded) = general_purpose::STANDARD.decode(&encoded) {
            if let Ok(decoded_str) = String::from_utf8(decoded) {
                let parts: Vec<&str> = decoded_str.split(':').collect();
                if parts.len() == 2 {
                    let username = parts[0];
                    let password = parts[1];
                    // Replace these with your actual username and password
                    if username == "admin" && password == "password" {
                        return Ok(());
                    }
                }
            }
        }
    }
    Err(HttpResponse::Unauthorized().body("Unauthorized"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let os_type = sys_info::os_type().unwrap();
    let eyes = EyesState {
        eyes_io: Arc::new(Mutex::new(None)),
        status: Arc::new(Mutex::new(false)),
    };

    let data = web::Data::new(AppState {
        eyes,
        current_camera_index: Arc::new(Mutex::new(None)),
        os_type,
    });

    let server_addr = "0.0.0.0";
    // let server_addr = "127.0.0.1";
    let server_port = 8081;

    let app = HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec!["Content-Type", "Authorization"])
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(data.clone())
            .route("/sensors/eyes/ws", web::get().to(sensors_eyes_event_web_socket))
            .route("/sensors/eyes", web::post().to(turn_eyes_on_off))
            .route("/system", web::get().to(get_system_info))
    })
        .bind((server_addr, server_port))?
        .run();

    println!("Server running at http://{server_addr}:{server_port}");
    app.await
}