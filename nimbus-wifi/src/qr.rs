use qrcode::render::svg;
use qrcode::render::unicode;
use qrcode::QrCode;

use nimbus_core::error::{NimbusError, Result};
use nimbus_core::types::Security;

/// A QR code rendered with Unicode half-blocks, for printing in a terminal.
pub fn generate_wifi_qr_unicode(
    ssid: &str,
    password: &str,
    security: &Security,
    hidden: bool,
) -> Result<String> {
    let code = QrCode::new(wifi_string(ssid, password, security, hidden))
        .map_err(|e| NimbusError::InvalidValue(format!("QR code generation failed: {}", e)))?;

    Ok(code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build())
}

pub fn generate_wifi_qr(
    ssid: &str,
    password: &str,
    security: &Security,
    hidden: bool,
) -> Result<String> {
    let code = QrCode::new(wifi_string(ssid, password, security, hidden))
        .map_err(|e| NimbusError::InvalidValue(format!("QR code generation failed: {}", e)))?;

    let svg = code.render::<svg::Color>().min_dimensions(200, 200).build();

    Ok(svg)
}

fn wifi_string(ssid: &str, password: &str, security: &Security, hidden: bool) -> String {
    let auth_type = match security {
        Security::Wpa2 => "WPA",
        Security::Wpa3 => "SAE",
        Security::Wpa2Wpa3Transition => "WPA",
        Security::Open => "nopass",
    };

    let escaped_ssid = escape_wifi_string(ssid);
    let escaped_pass = escape_wifi_string(password);

    format!(
        "WIFI:T:{};S:{};P:{};H:{};;",
        auth_type, escaped_ssid, escaped_pass, hidden
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_protects_the_wifi_payload() {
        assert_eq!(
            escape_wifi_string("a;b,c:d\"e\\f"),
            "a\\;b\\,c\\:d\\\"e\\\\f"
        );
    }

    #[test]
    fn an_open_network_asks_for_no_password() {
        let payload = wifi_string("Cafe", "", &Security::Open, false);
        assert_eq!(payload, "WIFI:T:nopass;S:Cafe;P:;H:false;;");
    }

    #[test]
    fn wpa3_uses_sae() {
        let payload = wifi_string("Home", "secret123", &Security::Wpa3, true);
        assert!(payload.starts_with("WIFI:T:SAE;"));
        assert!(payload.ends_with(";H:true;;"));
    }

    #[test]
    fn the_terminal_qr_renders_block_characters() {
        let qr = generate_wifi_qr_unicode(
            "Nimbus",
            "password123",
            &Security::Wpa2Wpa3Transition,
            false,
        )
        .expect("QR generation should succeed");

        assert!(qr.contains('\u{2588}') || qr.contains('\u{2580}'));
    }

    #[test]
    fn the_svg_qr_starts_with_an_svg_element() {
        let svg = generate_wifi_qr("Nimbus", "password123", &Security::Wpa2, false).unwrap();
        assert!(svg.contains("<svg"));
    }
}
