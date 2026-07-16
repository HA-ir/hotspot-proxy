#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs;
use crate::linux::detect_wifi_interface;
use std::process::Command;
use std::collections::{HashMap, HashSet};

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

// Read DHCP leases for hostname enrichment only (mac -> hostname).
fn read_dhcp_hostnames(wifi_if: &str) -> HashMap<String, String> {
    let lease_path = format!("/var/lib/NetworkManager/dnsmasq-{}.leases", wifi_if);
    let content = match fs::read_to_string(&lease_path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut map = HashMap::new();
    for line in content.lines() {
        // format: <expiry-epoch> <mac> <ip> <hostname-or-*> <client-id>
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 { continue; }
        let expiry: u64 = match parts[0].parse() { Ok(v) => v, Err(_) => continue };
        if expiry != 0 && expiry < now { continue; }
        let mac = parts[1].to_lowercase();
        if parts[3] != "*" {
            map.insert(mac, parts[3].to_string());
        }
    }
    map
}

// Probe whether an IP is still reachable by sending one ICMP ping (1s timeout).
fn is_reachable(ip: &str, iface: &str) -> bool {
    Command::new("ping")
        .args(["-c", "1", "-W", "1", "-I", iface, "-n", ip])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tauri::command]
fn get_connected_devices() -> Result<Vec<crate::Device>, String> {
    let wifi_if = detect_wifi_interface()?;
    let hostnames = read_dhcp_hostnames(&wifi_if);

    // Step 1: collect candidate IPs from the ARP table (exclude definitively dead states).
    let arp_out = Command::new("ip")
        .args(["neigh", "show", "dev", &wifi_if])
        .output()
        .map_err(|e| format!("Failed to run 'ip neigh': {}", e))?;

    let mut candidates: Vec<(String, String)> = Vec::new(); // (ip, mac)
    let mut seen_macs = HashSet::new();

    if arp_out.status.success() {
        let stdout = String::from_utf8_lossy(&arp_out.stdout);
        for line in stdout.lines() {
            if line.contains("FAILED") || line.contains("INCOMPLETE") || line.contains("NOARP") {
                continue;
            }
            // format: <ip> dev <iface> lladdr <mac> <STATE>
            let parts: Vec<&str> = line.split_whitespace().collect();
            // find "lladdr" token and take the next token as MAC
            if let Some(idx) = parts.iter().position(|&t| t == "lladdr") {
                if let Some(mac_str) = parts.get(idx + 1) {
                    let ip = parts[0].to_string();
                    let mac = mac_str.to_lowercase();
                    if seen_macs.insert(mac.clone()) {
                        candidates.push((ip, mac));
                    }
                }
            }
        }
    }

    // Step 2: probe each candidate in parallel, keep only live ones.
    // For typical hotspot sizes (1–10 devices) this takes ~500 ms worst-case.
    use std::thread;
    let wifi_if_clone = wifi_if.clone();
    let handles: Vec<_> = candidates
        .into_iter()
        .map(|(ip, mac)| {
            let iface = wifi_if_clone.clone();
            thread::spawn(move || {
                if is_reachable(&ip, &iface) { Some((ip, mac)) } else { None }
            })
        })
        .collect();

    let mut devices: Vec<crate::Device> = Vec::new();
    for h in handles {
        if let Some((ip, mac)) = h.join().unwrap_or(None) {
            let name = hostnames.get(&mac).cloned().unwrap_or_else(|| "Unknown Device".to_string());
            devices.push(crate::Device { ip, mac, name });
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