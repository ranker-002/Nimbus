use qrcode::QrCode;
use qrcode::render::svg;

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::Security;

pub fn generate_wifi_qr(
    ssid: &str,
    password: &str,
    security: &Security,
    hidden: bool,
) -> Result<String> {
    let auth_type = match security {
        Security::Wpa2 => "WPA",
        Security::Wpa3 => "SAE",
        Security::Wpa2Wpa3Transition => "WPA",
        Security::Open => "nopass",
    };

    let escaped_ssid = escape_wifi_string(ssid);
    let escaped_pass = escape_wifi_string(password);

    let wifi_string = format!(
        "WIFI:T:{};S:{};P:{};H:{};;",
        auth_type, escaped_ssid, escaped_pass, hidden
    );

    let code = QrCode::new(&wifi_string)
        .map_err(|e| NimbusError::InvalidValue(format!("QR code generation failed: {}", e)))?;

    let svg = code
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .build();

    Ok(svg)
}

fn escape_wifi_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            ',' => result.push_str("\\,"),
            ';' => result.push_str("\\;"),
            ':' => result.push_str("\\:"),
            '"' => result.push_str("\\\""),
            _ => result.push(c),
        }
    }
    result
}
