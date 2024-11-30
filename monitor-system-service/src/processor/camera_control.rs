use opencv::{
    prelude::*,
    videoio,
    core,
    Result,
};
use tokio::sync::{mpsc, watch};
use std::time::Duration;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

#[derive(Debug)]
pub enum CameraControl {
    Start,
    Stop,
}

pub struct CameraServer {
    frame_sender: mpsc::UnboundedSender<String>,
    stop_rx: watch::Receiver<bool>,
    running: watch::Receiver<bool>,
}

impl CameraServer {
    pub fn new(
        frame_sender: mpsc::UnboundedSender<String>,
        stop_rx: watch::Receiver<bool>,
        running: watch::Receiver<bool>,
    ) -> Self {
        Self {
            frame_sender,
            stop_rx,
            running,
        }
    }

    fn configure_camera(cap: &mut videoio::VideoCapture) -> Result<()> {
        let _ = cap.set(videoio::CAP_PROP_FOURCC,
                        videoio::VideoWriter::fourcc('M', 'J', 'P', 'G')? as f64);

        cap.set(videoio::CAP_PROP_FRAME_WIDTH, 320.0)?;
        cap.set(videoio::CAP_PROP_FRAME_HEIGHT, 240.0)?;
        cap.set(videoio::CAP_PROP_FPS, 15.0)?;
        cap.set(videoio::CAP_PROP_BUFFERSIZE, 1.0)?;
        let _ = cap.set(videoio::CAP_PROP_AUTO_EXPOSURE, 0.0);
        let _ = cap.set(videoio::CAP_PROP_AUTOFOCUS, 0.0);

        Ok(())
    }

    fn try_open_camera() -> Result<videoio::VideoCapture> {
        let mut cap = videoio::VideoCapture::new(0, videoio::CAP_V4L2)?;

        if !cap.is_opened()? {
            return Err(opencv::Error::new(
                core::StsError,
                "Failed to open camera".to_string(),
            ));
        }

        Self::configure_camera(&mut cap)?;

        let mut frame = core::Mat::default();
        for _ in 0..5 {
            cap.read(&mut frame)?;
            std::thread::sleep(Duration::from_millis(100));
        }

        Ok(cap)
    }

    pub async fn start_capture(&self) -> Result<()> {
        let mut cam = Self::try_open_camera()?;
        let mut frame = core::Mat::default();
        let mut consecutive_failures = 0;
        const MAX_FAILURES: i32 = 3;

        while *self.running.borrow() {
            if *self.stop_rx.borrow() {
                println!("Camera capture stopping due to stop signal");
                break;
            }

            let read_result = cam.read(&mut frame);
            match read_result {
                Ok(true) => {
                    if frame.empty() {
                        println!("Empty frame received");
                        consecutive_failures += 1;
                    } else {
                        consecutive_failures = 0;

                        let mut buffer = core::Vector::new();
                        let mut params = core::Vector::new();
                        params.push(opencv::imgcodecs::IMWRITE_JPEG_QUALITY);
                        params.push(60);

                        if let Ok(_) = opencv::imgcodecs::imencode(".jpg", &frame, &mut buffer, &params) {
                            let frame_data = BASE64.encode(&buffer);
                            if self.frame_sender.send(frame_data).is_err() {
                                println!("Frame receiver disconnected");
                                break;
                            }
                        }
                    }
                }
                Ok(false) | Err(_) => {
                    println!("Failed to read frame");
                    consecutive_failures += 1;
                }
            }

            if consecutive_failures >= MAX_FAILURES {
                if *self.stop_rx.borrow() {
                    break;
                }

                println!("Too many consecutive failures, reinitializing camera...");
                match Self::try_open_camera() {
                    Ok(new_cam) => {
                        cam = new_cam;
                        consecutive_failures = 0;
                        println!("Camera reinitialized successfully");
                    }
                    Err(e) => {
                        println!("Failed to reinitialize camera: {}", e);
                        break;
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(66)).await;
        }

        println!("Camera capture ended");
        Ok(())
    }
}