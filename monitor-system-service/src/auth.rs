use base64::engine::general_purpose;
use base64::Engine;

pub fn authenticate_basic(auth_str: &str) -> Result<(), String> {
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
    Err("Unauthorized".to_string())
}