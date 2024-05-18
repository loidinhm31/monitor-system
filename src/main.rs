use std::sync::{Arc, Mutex};

use actix_cors::Cors;
use actix_web::{App, Error, HttpRequest, HttpResponse, HttpServer, web};
use base64::{Engine as _, engine::general_purpose};
use futures_util::stream::Stream;
use futures_util::StreamExt;
use opencv::core::Vector;
use opencv::prelude::*;
use opencv::videoio;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ControlMessage {
    command: String,
}

struct AppState {
    eyes: Arc<Mutex<Option<videoio::VideoCapture>>>,
}

fn eyes_stream(state: web::Data<AppState>) -> impl Stream<Item=Result<web::Bytes, Error>> {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let state = state.clone();

    async_stream::stream! {
        loop {
            interval.tick().await;
            let mut eyes_guard = state.eyes.lock().unwrap();
            if let Some(ref mut eyes) = *eyes_guard {
                let mut frame = Mat::default();
                if eyes.read(&mut frame).is_ok() {
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

async fn turn_eyes_on(r: HttpRequest, data: web::Data<AppState>) -> HttpResponse {
    if let Err(e) = authenticate(&r) {
        return e;
    }
    let mut eyes_guard = data.eyes.lock().unwrap();
    if eyes_guard.is_none() {
        *eyes_guard = Some(videoio::VideoCapture::new(2, videoio::CAP_ANY).unwrap());
        if !eyes_guard.as_ref().unwrap().is_opened().unwrap() {
            *eyes_guard = None;
            return HttpResponse::InternalServerError().body("Unable to open eyes");
        }
    }
    HttpResponse::Ok().body("eyes turned on")
}

async fn turn_eyes_off(r: HttpRequest, data: web::Data<AppState>) -> HttpResponse {
    if let Err(e) = authenticate(&r) {
        return e;
    }
    let mut eyes_guard = data.eyes.lock().unwrap();
    *eyes_guard = None;
    HttpResponse::Ok().body("eyes turned off")
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
    let eyes = web::Data::new(AppState {
        eyes: Arc::new(Mutex::new(None)),
    });

    // let server_addr = "0.0.0.0";
    let server_addr = "127.0.0.1";
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
            .route("/sensors/eyes/on", web::post().to(turn_eyes_on))
            .route("/sensors/eyes/off", web::post().to(turn_eyes_off))
    })
        .bind((server_addr, server_port))?
        .run();

    println!("Server running at http://{server_addr}:{server_port}/");
    app.await
}
