use std::sync::{Arc, Mutex};

use actix_cors::Cors;
use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, web};
use base64::{Engine as _, engine::general_purpose};
use futures_util::stream::Stream;
use opencv::core::Vector;
use opencv::prelude::*;
use opencv::videoio;
use serde::{Deserialize, Serialize};
use sys_info;

#[derive(Serialize, Deserialize)]
struct ControlMessage {
    command: String,
}

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

fn eyes_stream(state: web::Data<AppState>) -> impl Stream<Item=Result<web::Bytes, Error>> {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let state = state.clone();

    async_stream::stream! {
        loop {
            interval.tick().await;
            let mut eyes_guard = state.eyes.eyes_io.lock().unwrap();
            if let Some(ref mut eyes_io) = *eyes_guard {
                let mut frame = Mat::default();
                if eyes_io.read(&mut frame).is_ok() {
                    let mut buf = Vector::new();
                    if opencv::imgcodecs::imencode(".jpg", &frame, &mut buf, &Vector::new()).is_ok() {
                        yield Ok(web::Bytes::from(format!("data: {}\n\n", general_purpose::STANDARD.encode(&buf))));
                    }
                }
            }
        }
    }
}

async fn sensors_eyes_event_stream(data: web::Data<AppState>) -> Result<HttpResponse, Error> {
    println!("Streaming eyes data...");

    let stream = eyes_stream(data);

    Ok(HttpResponse::Ok()
        .append_header(("Access-Control-Allow-Credentials", "true"))
        .content_type("text/event-stream")
        .streaming(stream))
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
                        println!("Running eyes index: {:?}", data.current_camera_index.lock().unwrap());
                    }
                }
            }
            HttpResponse::BadRequest().body("Wrong status of eyes")
        }
    } else {
        let mut eyes_guard = data.eyes.eyes_io.lock().unwrap();
        *eyes_guard = None;
        *data.eyes.status.lock().unwrap() = false;
        HttpResponse::Ok().body("Eyes turned off")
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

fn authenticate(req: &HttpRequest) -> Result<(), HttpResponse> {
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
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
        }
    }
    Err(HttpResponse::Unauthorized().body("Unauthorized"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let os_type = sys_info::os_type().unwrap_or_else(|_| "Unknown".to_string());

    let eyes = web::Data::new(AppState {
        eyes: EyesState {
            eyes_io: Arc::new(Mutex::new(None)),
            status: Arc::new(Mutex::new(false)),
        },
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
            .app_data(eyes.clone())
            .wrap(cors)
            .route("/sensors/eyes/event", web::get().to(sensors_eyes_event_stream))
            .route("/sensors/eyes", web::post().to(turn_eyes_on_off))
            .route("/system", web::get().to(get_system_info))
    })
        .bind((server_addr, server_port))?
        .run();

    println!("Server running at http://{server_addr}:{server_port}");
    app.await
}
