use actix::{Actor, StreamHandler, AsyncContext};
use actix_web::{web, App, HttpServer, HttpRequest, Error, HttpResponse};
use actix_web::web::Payload;
use actix_cors::Cors;
use actix_web_actors::ws;
use opencv::prelude::*;
use opencv::videoio;
use std::sync::{Mutex, Arc};
use opencv::core::Vector;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct ControlMessage {
    command: String,
}

struct AppState {
    camera: Arc<Mutex<Option<videoio::VideoCapture>>>,
}

struct WebSocketSession {
    state: web::Data<AppState>,
}

impl Actor for WebSocketSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let state = self.state.clone();
        ctx.run_interval(std::time::Duration::from_millis(100), move |act, ctx| {
            let mut camera_guard = state.camera.lock().unwrap();
            if let Some(ref mut camera) = *camera_guard {
                let mut frame = Mat::default();
                if camera.read(&mut frame).is_ok() {
                    let mut buf = Vector::new();
                    if opencv::imgcodecs::imencode(".jpg", &frame, &mut buf, &Vector::new()).is_ok() {
                        ctx.binary(buf.to_vec());
                    }
                }
            }
        });
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebSocketSession {
    fn handle(&mut self, _: Result<ws::Message, ws::ProtocolError>, _: &mut Self::Context) {}
}

async fn ws_index(r: HttpRequest, stream: Payload, data: web::Data<AppState>) -> Result<HttpResponse, Error> {
    ws::start(WebSocketSession { state: data }, &r, stream)
}

async fn turn_camera_on(data: web::Data<AppState>) -> HttpResponse {
    let mut camera_guard = data.camera.lock().unwrap();
    if camera_guard.is_none() {
        *camera_guard = Some(videoio::VideoCapture::new(0, videoio::CAP_V4L2).unwrap());
        if !camera_guard.as_ref().unwrap().is_opened().unwrap() {
            *camera_guard = None;
            return HttpResponse::InternalServerError().body("Unable to open camera");
        }
    }
    HttpResponse::Ok().body("Camera turned on")
}

async fn turn_camera_off(data: web::Data<AppState>) -> HttpResponse {
    let mut camera_guard = data.camera.lock().unwrap();
    *camera_guard = None;
    HttpResponse::Ok().body("Camera turned off")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let camera = web::Data::new(AppState {
        camera: Arc::new(Mutex::new(None)),
    });

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allowed_methods(vec!["GET", "POST"])
            .allowed_headers(vec!["Content-Type"])
            .max_age(3600);

        App::new()
            .app_data(camera.clone())
            .wrap(cors)
            .route("/ws/", web::get().to(ws_index))
            .route("/camera/on", web::post().to(turn_camera_on))
            .route("/camera/off", web::post().to(turn_camera_off))
    })
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}
