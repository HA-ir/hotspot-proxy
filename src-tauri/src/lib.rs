#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Clone, Debug)]
pub struct Device {
    ip: String,
    mac: String,
    name: String,
}

fn validate_hotspot_inputs(ssid: &str, password: &str) -> Result<(), String> {
    if ssid.trim().is_empty() || ssid.len() > 32 {
        return Err("SSID must be 1-32 characters.".into());
    }
    if password.len() < 8 || password.len() > 63 {
        return Err("Password must be 8-63 characters (WPA2 requirement).".into());
    }
    Ok(())
}

/// Read the hotspot's own DHCP lease file. NetworkManager runs a dedicated dnsmasq
/// instance for `ipv4.method=shared` connections and writes leases here the instant
/// a client gets an IP — far more immediate and reliable than waiting for that client
/// to show up in the ARP/neighbor cache.
fn read_dhcp_leases(wifi_if: &str) -> Vec<(String, String, String)> {
    let lease_path = format!("/var/lib/NetworkManager/dnsmasq-{}.leases", wifi_if);
    let content = match fs::read_to_string(&lease_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(), // no leases yet, or path differs on this distro — fall back to ARP only
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut leases = Vec::new();
    for line in content.lines() {
        // format: <expiry-epoch> <mac> <ip> <hostname-or-*> <client-id>
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        let expiry: u64 = match parts[0].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if expiry != 0 && expiry < now {
            continue; // expired lease, client is gone
        }
        let mac = parts[1].to_lowercase();
        let ip = parts[2].to_string();
        let hostname = if parts[3] == "*" { "Unknown Device".to_string() } else { parts[3].to_string() };
        leases.push((mac, ip, hostname));
    }
    leases
}

#[tauri::command]
pub fn get_connected_devices() -> Result<Vec<crate::Device>, String> {
    let wifi_if = detect_wifi_interface()?;
    let mut devices: Vec<crate::Device> = Vec::new();
    let mut seen_macs = std::collections::HashSet::new();

    // Primary source: DHCP leases. Catches a client the moment it joins.
    for (mac, ip, name) in read_dhcp_leases(&wifi_if) {
        if seen_macs.insert(mac.clone()) {
            devices.push(crate::Device { ip, mac, name });
        }
    }

    // Secondary source: ARP/neighbor table, to catch anything with a static IP
    // that never went through DHCP. Failure here is non-fatal — leases alone
    // are enough for the common case, so we don't error out the whole call.
    if let Ok(output) = Command::new("ip").args(["neigh", "show", "dev", &wifi_if]).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("FAILED") || line.contains("INCOMPLETE") {
                    continue;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 && parts[1] == "lladdr" {
                    let mac = parts[2].to_lowercase();
                    if seen_macs.insert(mac.clone()) {
                        devices.push(crate::Device {
                            ip: parts[0].to_string(),
                            mac,
                            name: "Unknown Device".to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(devices)
}

#[tauri::command]
fn start_hotspot(
    app: tauri::AppHandle,
    ssid: String,
    password: String,
    proxy_url: String,
) -> Result<String, String> {
    validate_hotspot_inputs(&ssid, &password)?;

    #[cfg(target_os = "linux")]
    {
        linux::start(&app, &ssid, &password, &proxy_url)
    }

    #[cfg(target_os = "windows")]
    {
        windows::start(&app, &ssid, &password, &proxy_url)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported operating system".into())
    }
}

#[tauri::command]
fn stop_hotspot() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        linux::stop()
    }

    #[cfg(target_os = "windows")]
    {
        windows::stop()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported operating system".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![start_hotspot, stop_hotspot, get_connected_devices])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}