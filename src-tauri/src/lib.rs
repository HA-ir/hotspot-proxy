#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

use std::process::Command;
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
struct Device {
    ip: String,
    mac: String,
    name: String,
}

#[tauri::command]
fn get_connected_devices() -> Result<Vec<Device>, String> {
    let output = Command::new("ip")
        .args(["neigh", "show", "dev", "wlp0s20f3"])
        .output()
        .map_err(|e| e.to_string())?;
        
    if !output.status.success() {
        return Err("Failed to get connected devices".into());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    
    for line in stdout.lines() {
        // Example line: 10.42.0.122 lladdr 12:34:56:78:9a:bc STALE
        if line.contains("REACHABLE") || line.contains("STALE") || line.contains("DELAY") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "lladdr" {
                let ip = parts[0].to_string();
                let mac = parts[2].to_string();
                
                // For name, we try to resolve hostname via avahi or leave as Unknown
                // To keep it fast and avoid hanging, we'll assign "Unknown Device" for now
                let name = "Unknown Device".to_string();
                
                devices.push(Device { ip, mac, name });
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
