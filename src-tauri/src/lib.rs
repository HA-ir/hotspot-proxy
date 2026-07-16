#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct Device {
    pub ip: String,
    pub mac: String,
    pub name: String,
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

#[tauri::command]
fn check_wintun() -> bool {
    #[cfg(target_os = "windows")]
    { windows::wintun_exists() }
    #[cfg(not(target_os = "windows"))]
    { true }
}

#[tauri::command]
fn download_wintun() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    { windows::download_wintun() }
    #[cfg(not(target_os = "windows"))]
    { Ok("Not needed on this platform.".into()) }
}

#[tauri::command]
fn get_connected_devices() -> Result<Vec<crate::Device>, String> {
    #[cfg(target_os = "linux")]
    {
        linux::get_connected_devices()
    }

    #[cfg(target_os = "windows")]
    {
        windows::get_connected_devices()
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Err("Unsupported operating system".into())
    }
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
        .invoke_handler(tauri::generate_handler![start_hotspot, stop_hotspot, get_connected_devices, check_wintun, download_wintun])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
