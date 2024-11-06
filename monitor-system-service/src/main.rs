use std::sync::{Arc, Mutex};

use actix_cors::Cors;
use actix_web::web::Payload;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use opencv::prelude::*;
use opencv::videoio;
use sys_info;

use crate::models::{EyeInfo, EyeRequest, EyesState, SystemInfo};
use crate::websocket::{AudioWebSocketSession, WebSocketSession};

mod auth;
mod camera;
mod audio;
mod models;
mod websocket;

struct AppState {
    eyes: EyesState,
    current_camera_index: Arc<Mutex<Option<i32>>>,
    os_type: String,
}


async fn sensors_eyes_event_web_socket(r: HttpRequest, stream: Payload, data: web::Data<AppState>) -> Result<HttpResponse, Error> {
    ws::start(WebSocketSession { state: data, authenticated: false }, &r, stream)
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

async fn audio_event_web_socket(r: HttpRequest, stream: Payload) -> Result<HttpResponse, Error> {
    ws::start(AudioWebSocketSession { authenticated: false, audio_stream: None, audio_streaming: false, audio_buffer: Arc::new(Mutex::new(vec![])) }, &r, stream)
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
            .route("/healthz", web::get().to(|| async { HttpResponse::Ok().body("Up") }))
            .route("/sensors/eyes/ws", web::get().to(sensors_eyes_event_web_socket))
            .route("/sensors/ears/ws", web::get().to(audio_event_web_socket))
            .route("/sensors/eyes", web::post().to(turn_eyes_on_off))
            .route("/system", web::get().to(get_system_info))
    })
        .bind((server_addr, server_port))?
        .run();

    println!("Server running at http://{server_addr}:{server_port}");
    app.await
}
