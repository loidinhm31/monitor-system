use std::sync::Mutex;

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use opencv::core::Vector;
use opencv::prelude::*;
use opencv::videoio;

struct AppState {
    camera: Mutex<videoio::VideoCapture>,
}

async fn stream_video(data: web::Data<AppState>) -> impl Responder {
    let mut camera = data.camera.lock().unwrap();
    let mut frame = Mat::default();

    if camera.read(&mut frame).is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    let mut buf = Vector::new();
    if opencv::imgcodecs::imencode(".jpg", &frame, &mut buf, &Vector::new()).is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok()
        .content_type("image/jpeg")
        .body(buf.to_vec())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let camera = videoio::VideoCapture::new(0, videoio::CAP_ANY).unwrap();
    if !camera.is_opened().unwrap() {
        panic!("Unable to open camera");
    }

    let camera = web::Data::new(AppState {
        camera: Mutex::new(camera),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(camera.clone())
            .route("/stream", web::get().to(stream_video))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
