use actix::{Actor, ActorContext, AsyncContext, Handler, Message, StreamHandler};
use actix_web_actors::ws;
use cpal::{SampleFormat, Stream};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crate::auth;
use crate::websocket::AudioWebSocketSession;

pub struct AudioData(pub Vec<u8>);


impl Message for AudioData {
    type Result = ();
}

impl Actor for AudioWebSocketSession {
    type Context = ws::WebsocketContext<Self>;
}


impl Handler<AudioData> for AudioWebSocketSession {
    type Result = ();

    fn handle(&mut self, msg: AudioData, ctx: &mut Self::Context) {
        ctx.binary(msg.0);
    }

}


impl AudioWebSocketSession {
    fn start_audio_stream(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        let audio_buffer = self.audio_buffer.clone();

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
                        let mut buffer = audio_buffer.lock().unwrap();
                        buffer.extend(audio_data);
                        if buffer.len() >= 1024 {
                            addr.do_send(AudioData(buffer.split_off(0)));
                        }
                    },
                    |err| {
                        eprintln!("Error occurred on audio input stream: {:?}", err);
                    }
                ).unwrap();
                stream.play().unwrap();
                self.audio_stream = Some(stream);
                self.audio_streaming = true;
            },
            _ => panic!("Unsupported sample format"),
        }
    }

    fn stop_audio_stream(&mut self) {
        if let Some(stream) = self.audio_stream.take() {
            stream.pause().unwrap();
            self.audio_streaming = false;
        }
    }
}


impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for AudioWebSocketSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        println!("Audio Message: {:?}", msg);

        match msg {
            Ok(ws::Message::Text(text)) => {
                if !self.authenticated {
                    if auth::authenticate_basic(&text).is_ok() {
                        self.authenticated = true;
                        ctx.text("Authenticated");
                    } else {
                        ctx.text("Unauthorized");
                        ctx.stop();
                    }
                } else {
                    match text.to_string().as_str() {
                        "start_audio" => {
                            if !self.audio_streaming {
                                self.start_audio_stream(ctx);
                            }
                        }
                        "stop_audio" => {
                            if self.audio_streaming {
                                self.stop_audio_stream();
                            }
                        }
                        _ => (),
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => (),
        }
    }
}
