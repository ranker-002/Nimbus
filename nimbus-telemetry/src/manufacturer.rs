use std::collections::HashMap;
use std::sync::OnceLock;

use mac_address::MacAddress;

static OUI_DB: OnceLock<HashMap<[u8; 3], String>> = OnceLock::new();

fn load_oui_db() -> &'static HashMap<[u8; 3], String> {
    OUI_DB.get_or_init(|| {
        let mut db = HashMap::new();
        let oui_entries = [
            ([0x00, 0x50, 0x56], "VMware"),
            ([0x00, 0x0C, 0x29], "VMware"),
            ([0x00, 0x1C, 0x42], "Parallels"),
            ([0x08, 0x00, 0x27], "VirtualBox"),
            ([0x52, 0x54, 0x00], "QEMU/KVM"),
            ([0x00, 0x16, 0x3E], "Xen"),
            ([0x00, 0x15, 0x5D], "Hyper-V"),
            ([0x00, 0x03, 0xFF], "Microsoft"),
            ([0x00, 0x1A, 0x11], "Google"),
            ([0x3C, 0x5A, 0xB4], "Google"),
            ([0xF4, 0xF5, 0xE8], "Google"),
            ([0x00, 0x17, 0xF2], "Apple"),
            ([0x3C, 0x22, 0xFB], "Apple"),
            ([0xA4, 0x83, 0xE7], "Apple"),
            ([0xDC, 0xA6, 0x32], "Raspberry Pi"),
            ([0xB8, 0x27, 0xEB], "Raspberry Pi"),
            ([0x28, 0xCD, 0xC1], "Raspberry Pi"),
            ([0x00, 0x1E, 0x58], "D-Link"),
            ([0x1C, 0x5F, 0x2B], "D-Link"),
            ([0x00, 0x26, 0x5A], "D-Link"),
            ([0x00, 0x1D, 0xD8], "Tenda"),
            ([0xC8, 0x3A, 0x35], "Tenda"),
            ([0x00, 0x0E, 0x8F], "Netgear"),
            ([0x20, 0xE5, 0x2A], "Netgear"),
            ([0x44, 0x94, 0xFC], "Netgear"),
            ([0x00, 0x1A, 0x2B], "Cisco"),
            ([0x00, 0x26, 0x0B], "Cisco"),
            ([0x68, 0x86, 0xA7], "Cisco"),
            ([0x00, 0x1B, 0x2F], "TP-Link"),
            ([0x50, 0xC7, 0xBF], "TP-Link"),
            ([0x14, 0xCC, 0x20], "TP-Link"),
            ([0x00, 0x14, 0x6C], "Netis"),
            ([0x00, 0x22, 0x6B], "Samsung"),
            ([0x00, 0x1E, 0x65], "Samsung"),
            ([0x30, 0x96, 0xFB], "Samsung"),
            ([0x00, 0x1E, 0x7D], "Xiaomi"),
            ([0x64, 0xB4, 0x73], "Xiaomi"),
            ([0x7C, 0x1D, 0xD3], "Xiaomi"),
            ([0x00, 0x26, 0xAB], "Huawei"),
            ([0x48, 0x46, 0xFB], "Huawei"),
            ([0xE0, 0x24, 0x7F], "Huawei"),
            ([0xAC, 0x3B, 0x67], "OnePlus"),
            ([0xA0, 0x20, 0xA6], "OnePlus"),
            ([0x00, 0x1D, 0xA5], "Sony"),
            ([0xFC, 0x0F, 0xE6], "Sony"),
            ([0x00, 0x1F, 0x32], "HTC"),
            ([0x00, 0x1F, 0xB2], "Motorola"),
            ([0x3C, 0x8B, 0xFE], "Motorola"),
            ([0x00, 0x1C, 0xB3], "Apple"),
            ([0xF8, 0x1E, 0xDF], "Apple"),
            ([0xA4, 0x5E, 0x60], "Apple"),
            ([0x00, 0x25, 0x00], "Apple"),
            ([0x34, 0x36, 0x3B], "Apple"),
        ];

        for (prefix, vendor) in oui_entries {
            db.insert(prefix, vendor.to_string());
        }
        db
    })
}

pub fn lookup_manufacturer(mac: &MacAddress) -> Option<String> {
    let bytes = mac.bytes();
    let prefix = [bytes[0], bytes[1], bytes[2]];
    load_oui_db().get(&prefix).cloned()
}

pub fn signal_to_percent(signal_dbm: i32) -> u8 {
    (((signal_dbm as f64 + 100.0) * 2.0).round() as i32).clamp(0, 100) as u8
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_rate(bytes_per_sec: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec))
}
