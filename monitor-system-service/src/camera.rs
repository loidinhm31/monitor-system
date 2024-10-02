use actix::{Actor, ActorContext, AsyncContext, StreamHandler};
use actix_web_actors::ws;
use opencv::core::{Mat, Vector};
use opencv::imgcodecs;
use opencv::prelude::{VectorToVec, VideoCaptureTrait};
use crate::auth::authenticate_basic;
use crate::websocket::WebSocketSession;

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
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebSocketSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        println!("Video Message: {:?}", msg);
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
                } else {
                    // Handle other messages after authentication
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

