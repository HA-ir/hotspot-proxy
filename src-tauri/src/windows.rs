use std::process::Command;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn command_exists(cmd: &str) -> bool {
    Command::new("where")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn check_dependencies() -> Vec<crate::MissingDependency> {
    let mut missing = Vec::new();

    if !command_exists("powershell") {
        missing.push(crate::MissingDependency {
            name: "powershell".to_string(),
            install_command: None,
        });
    }

    if !command_exists("netsh") {
        missing.push(crate::MissingDependency {
            name: "netsh".to_string(),
            install_command: None,
        });
    }

    missing
}

fn run_cmd(cmd: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(cmd);
    for arg in args {
        command.arg(arg);
    }
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Err(if stderr.trim().is_empty() { stdout } else { stderr })
    }
}

/// Run a PowerShell script string. Inherits the process's elevation level.
fn run_powershell(script: &str) -> Result<String, String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy", "Bypass",
            "-Command", script,
        ])
        .output()
        .map_err(|e| format!("Failed to launch PowerShell: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(if stderr.trim().is_empty() { stdout } else { stderr })
    }
}

/// Escape a string for safe embedding inside a PowerShell double-quoted string.
/// Escapes backtick, dollar, double-quote, and single-quote.
fn ps_escape(s: &str) -> String {
    s.replace('`', "``")
        .replace('$', "`$")
        .replace('"', "`\"")
        .replace('\'', "''")
}

// ---------------------------------------------------------------------------
// Wi-Fi interface detection
// ---------------------------------------------------------------------------

pub fn detect_wifi_interface() -> Result<String, String> {
    let out = run_cmd("netsh", &["wlan", "show", "interfaces"])?;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Name") {
            if let Some(name) = trimmed.splitn(2, ':').nth(1) {
                return Ok(name.trim().to_string());
            }
        }
    }
    Err("No Wi-Fi interface found. Make sure Wi-Fi is enabled.".into())
}

// ---------------------------------------------------------------------------
// Connected device discovery
// ---------------------------------------------------------------------------

pub fn get_connected_devices() -> Result<Vec<crate::Device>, String> {
    // Windows Mobile Hotspot uses ICS, which serves 192.168.137.0/24.
    // `arp -a` lists the ARP cache; we look for the section covering that subnet.
    let out = run_cmd("arp", &["-a"])?;

    let mut in_hotspot_section = false;
    let mut candidates: Vec<(String, String)> = Vec::new();
    let mut seen_macs = HashSet::new();

    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Interface:") {
            in_hotspot_section = trimmed.contains("192.168.137.");
            continue;
        }
        if !in_hotspot_section || trimmed.is_empty() || trimmed.starts_with("Internet") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 3 { continue; }
        let ip = parts[0].to_string();
        let mac = parts[1].replace('-', ":").to_lowercase();
        let entry_type = parts[2];

        if entry_type != "dynamic" { continue; }
        if ip.ends_with(".255") || ip == "192.168.137.1" { continue; }

        if seen_macs.insert(mac.clone()) {
            candidates.push((ip, mac));
        }
    }

    // Probe reachability in parallel with Windows-compatible ping flags.
    use std::thread;
    let handles: Vec<_> = candidates
        .into_iter()
        .map(|(ip, mac)| {
            thread::spawn(move || {
                let reachable = Command::new("ping")
                    .args(["-n", "1", "-w", "1000", "-4", &ip])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if reachable { Some((ip, mac)) } else { None }
            })
        })
        .collect();

    let mut devices: Vec<crate::Device> = Vec::new();
    for h in handles {
        match h.join() {
            Ok(Some((ip, mac))) => {
                devices.push(crate::Device { ip, mac, name: "Unknown Device".to_string() });
            }
            Ok(None) => {}
            Err(_) => eprintln!("ping thread panicked"),
        }
    }

    Ok(devices)
}

// ---------------------------------------------------------------------------
// Hotspot start / stop via WinRT NetworkOperatorTetheringManager
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// wintun.dll auto-install
// ---------------------------------------------------------------------------

pub fn wintun_path() -> std::path::PathBuf {
    let exe_path = std::env::current_exe().unwrap_or_default();
    exe_path.parent().unwrap_or(std::path::Path::new(".")).join("wintun.dll")
}

pub fn wintun_exists() -> bool {
    let p = wintun_path();
    if p.exists() { return true; }
    // dev-mode fallback: next to the tun2socks binary in src-tauri/bin/
    let dev = std::env::current_dir()
        .unwrap_or_default()
        .join("src-tauri").join("bin").join("wintun.dll");
    dev.exists()
}

pub fn download_wintun() -> Result<String, String> {
    let dest = wintun_path();
    let dest_str = dest.to_str().unwrap_or("wintun.dll");

    // wintun ships as a zip containing amd64/wintun.dll.
    // We download with PowerShell's built-in Invoke-WebRequest and extract with Expand-Archive.
    let script = format!(r#"
$ErrorActionPreference = 'Stop'
$zip = [System.IO.Path]::GetTempFileName() + '.zip'
Invoke-WebRequest -Uri 'https://www.wintun.net/builds/wintun-0.14.1.zip' -OutFile $zip -UseBasicParsing
$tmp = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), 'wintun-extract')
if (Test-Path $tmp) {{ Remove-Item $tmp -Recurse -Force }}
Expand-Archive -Path $zip -DestinationPath $tmp
Copy-Item -Path "$tmp\wintun\bin\amd64\wintun.dll" -Destination '{dest_str}' -Force
Remove-Item $zip -Force
Remove-Item $tmp -Recurse -Force
Write-Output "wintun.dll installed."
"#, dest_str = dest_str.replace('\'', "''"));

    run_powershell(&script)
}

pub fn start(
    _app: &tauri::AppHandle,
    ssid: &str,
    password: &str,
    proxy_url: &str,
) -> Result<String, String> {
    let ssid_e = ps_escape(ssid);
    let password_e = ps_escape(password);

    // Step 1 — Configure and start the Mobile Hotspot via WinRT.
    // NetworkOperatorTetheringManager is available on Windows 10 1607+.
    let hotspot_script = format!(r#"
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager, Windows.Networking.NetworkOperators, ContentType=WindowsRuntime]

function Await($task) {{
    $task.GetAwaiter().GetResult()
}}

$ifaces = [Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType=WindowsRuntime]::GetConnectionProfiles()
$profile = $ifaces | Where-Object {{ $_.IsWlanConnectionProfile }} | Select-Object -First 1
if (-not $profile) {{ throw "No Wi-Fi connection profile found." }}

$manager = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($profile)
$config  = $manager.GetCurrentAccessPointConfiguration()
$config.Ssid     = "{ssid_e}"
$config.Passphrase = "{password_e}"
Await($manager.ConfigureAccessPointAsync($config))
Await($manager.StartTetheringAsync())
Write-Output "Mobile Hotspot started."
"#);

    run_powershell(&hotspot_script)?;

    // Step 2 — Locate tun2socks binary.
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let bin_dir = exe_path.parent().unwrap();
    let mut tun2socks_path = bin_dir.join("tun2socks.exe");

    if !tun2socks_path.exists() {
        tun2socks_path = std::env::current_dir()
            .unwrap()
            .join("src-tauri")
            .join("bin")
            .join("tun2socks-x86_64-pc-windows-msvc.exe");
    }

    if !tun2socks_path.exists() {
        return Err("Bundled tun2socks binary not found. Place tun2socks-x86_64-pc-windows-msvc.exe in src-tauri/bin/.".to_string());
    }

    // wintun.dll must be present next to the app — tun2socks loads it at runtime.
    if !wintun_exists() {
        return Err("wintun.dll not found. Please allow the app to download it automatically.".to_string());
    }

    // Step 3 — Kill any stale tun2socks instance.
    let _ = run_cmd("taskkill", &["/F", "/IM", "tun2socks.exe"]);

    // Step 4 — Launch tun2socks (creates a WinTUN adapter named "tun0").
    Command::new(&tun2socks_path)
        .args(["--device", "tun0", "--proxy", proxy_url, "--loglevel", "info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to launch tun2socks: {}", e))?;

    // Give tun2socks time to create and register the WinTUN adapter.
    std::thread::sleep(std::time::Duration::from_millis(2000));

    // Step 5 — Assign an IP to the tun0 adapter.
    run_cmd("netsh", &[
        "interface", "ip", "set", "address",
        "name=tun0", "static", "10.0.0.1", "255.255.255.0",
    ])?;

    // Step 6 — Route the ICS hotspot subnet through tun0.
    let _ = run_cmd("route", &["delete", "192.168.137.0"]);
    run_cmd("route", &[
        "add", "192.168.137.0", "mask", "255.255.255.0",
        "10.0.0.1", "metric", "1",
    ])?;

    // Step 7 — Enable IP forwarding (persisted in registry, takes effect immediately).
    run_powershell(
        r#"Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters' -Name IPEnableRouter -Value 1"#,
    )?;

    Ok(format!(
        "Hotspot '{}' started. Client traffic is tunnelled through {}.",
        ssid, proxy_url
    ))
}

pub fn stop() -> Result<String, String> {
    // Stop tun2socks.
    let _ = run_cmd("taskkill", &["/F", "/IM", "tun2socks.exe"]);

    // Remove routing entry.
    let _ = run_cmd("route", &["delete", "192.168.137.0"]);

    // Disable IP forwarding.
    let _ = run_powershell(
        r#"Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters' -Name IPEnableRouter -Value 0"#,
    );

    // Stop Mobile Hotspot via WinRT.
    let stop_script = r#"
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager, Windows.Networking.NetworkOperators, ContentType=WindowsRuntime]

function Await($task) { $task.GetAwaiter().GetResult() }

$ifaces = [Windows.Networking.Connectivity.NetworkInformation, Windows.Networking.Connectivity, ContentType=WindowsRuntime]::GetConnectionProfiles()
$profile = $ifaces | Where-Object { $_.IsWlanConnectionProfile } | Select-Object -First 1
if ($profile) {
    $manager = [Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager]::CreateFromConnectionProfile($profile)
    Await($manager.StopTetheringAsync())
}
Write-Output "Mobile Hotspot stopped."
"#;
    run_powershell(stop_script)?;

    Ok("Hotspot stopped and routing cleaned up.".to_string())
}
