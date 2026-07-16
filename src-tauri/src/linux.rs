use std::process::Command;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Auto-detect the first Wi-Fi capable network device via NetworkManager.
/// Replaces the previous hardcoded "wlp0s20f3", which only worked on one machine.
pub fn detect_wifi_interface() -> Result<String, String> {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE", "device", "status"])
        .output()
        .map_err(|e| format!("Failed to run nmcli: {}", e))?;

    if !output.status.success() {
        return Err("Failed to query network devices via nmcli.".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let mut parts = line.splitn(2, ':');
        if let (Some(device), Some(dev_type)) = (parts.next(), parts.next()) {
            if dev_type == "wifi" {
                return Ok(device.to_string());
            }
        }
    }

    Err("No Wi-Fi interface found on this system.".into())
}

/// Wrap a string in single quotes for safe embedding in a POSIX shell script,
/// escaping any embedded single quotes. Neutralizes shell metacharacters
/// ($, `, ", ;, &&, etc.) regardless of what the user typed as SSID/password/proxy URL.
fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', r"'\''"))
}

/// Create a private (0700), randomly-named temp directory for the generated script,
/// instead of a predictable /tmp path any local user could race or symlink-attack.
fn make_private_temp_dir() -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("hotspot-proxy-{}-{}", std::process::id(), nanos));

    fs::create_dir(&dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("Failed to set temp dir permissions: {}", e))?;

    Ok(dir)
}

fn run_privileged_script(script: &str) -> Result<String, String> {
    let dir = make_private_temp_dir()?;
    let script_path = dir.join("run.sh");

    fs::write(&script_path, script).map_err(|e| format!("Failed to write script: {}", e))?;
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("Failed to set script permissions: {}", e))?;

    let output = Command::new("pkexec")
        .arg("bash")
        .arg(&script_path)
        .output()
        .map_err(|e| format!("Failed to execute pkexec: {}", e));

    let _ = fs::remove_dir_all(&dir);

    let output = output?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Exit Code: {:?}\n-- STDOUT --\n{}\n-- STDERR --\n{}", output.status.code(), stdout, stderr))
    }
}

pub fn start(_app: &tauri::AppHandle, ssid: &str, password: &str, proxy_url: &str) -> Result<String, String> {
    let wifi_if = detect_wifi_interface()?;
    let hotspot_con_name = "ProxyHotspot";
    let tun_if = "tun0";
    let tun_ip = "10.0.0.1";
    let hotspot_subnet = "10.42.0.0/24";
    let table_id = "100";

    let ssid_q = shell_quote(ssid);
    let password_q = shell_quote(password);
    let proxy_url_q = shell_quote(proxy_url);

    // Locate the sidecar binary
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let bin_dir = exe_path.parent().unwrap();
    let mut tun2socks_path = bin_dir.join("tun2socks");

    if !tun2socks_path.exists() {
        tun2socks_path = std::env::current_dir().unwrap().join("src-tauri").join("bin").join("tun2socks-x86_64-unknown-linux-gnu");
    }

    if !tun2socks_path.exists() {
        return Err("Bundled tun2socks binary not found!".to_string());
    }

    let tun2socks_bin_q = shell_quote(tun2socks_path.to_str().unwrap());

    let script = format!(r#"#!/bin/bash
set -e

echo "Cleaning up stale TUN..."
ip link del dev {tun_if} 2>/dev/null || true
pkill -f "tun2socks --device {tun_if}" || true
if [ -f /tmp/tun2socks.pid ]; then
    kill $(cat /tmp/tun2socks.pid) 2>/dev/null || true
    rm -f /tmp/tun2socks.pid
fi

echo "Setting up Hotspot..."
if ! nmcli con show "{hotspot_con_name}" > /dev/null 2>&1; then
    nmcli con add type wifi ifname {wifi_if} con-name "{hotspot_con_name}" autoconnect no ssid {ssid_q} > /dev/null
    nmcli con modify "{hotspot_con_name}" 802-11-wireless.mode ap 802-11-wireless.band bg ipv4.method shared ipv6.method disabled
    nmcli con modify "{hotspot_con_name}" wifi-sec.key-mgmt wpa-psk wifi-sec.psk {password_q}
fi
nmcli con up "{hotspot_con_name}" > /dev/null

echo "Setting up TUN interface..."
sysctl -w net.ipv4.ip_forward=1 > /dev/null
ip tuntap add mode tun dev {tun_if}
ip addr add {tun_ip}/24 dev {tun_if}
ip link set {tun_if} mtu 1500
ip link set {tun_if} up

echo "Starting tun2socks..."
{tun2socks_bin_q} --device {tun_if} --proxy {proxy_url_q} --loglevel debug > /var/log/tun2socks.log 2>&1 &
echo $! > /tmp/tun2socks.pid

echo "Applying routing and iptables..."
sysctl -w net.ipv4.conf.all.rp_filter=0 > /dev/null
sysctl -w net.ipv4.conf.{tun_if}.rp_filter=0 > /dev/null
sysctl -w net.ipv4.conf.{wifi_if}.rp_filter=0 > /dev/null

# All hotspot-subnet traffic (including DNS) is routed through tun0 -> tun2socks -> SOCKS5.
# No DNS-specific DNAT rule here on purpose: carving DNS out to a hardcoded resolver
# would leak every domain lookup outside the proxy tunnel.
ip rule add from {hotspot_subnet} table {table_id} 2>/dev/null || true
ip route add default dev {tun_if} table {table_id} 2>/dev/null || true
ip route flush cache

iptables -I FORWARD -i {tun_if} -j ACCEPT
iptables -I FORWARD -o {tun_if} -j ACCEPT
iptables -I FORWARD -i {wifi_if} -o {tun_if} -j ACCEPT
iptables -I FORWARD -i {tun_if} -o {wifi_if} -j ACCEPT

echo "Hotspot started successfully and routed through the proxy."
"#);

    run_privileged_script(&script)
}

pub fn stop() -> Result<String, String> {
    let wifi_if = detect_wifi_interface().unwrap_or_else(|_| "wlan0".to_string());
    let hotspot_con_name = "ProxyHotspot";
    let tun_if = "tun0";
    let hotspot_subnet = "10.42.0.0/24";
    let table_id = "100";

    let script = format!(r#"#!/bin/bash
set -e

echo "Stopping Hotspot..."
nmcli con down "{hotspot_con_name}" > /dev/null 2>&1 || true

echo "Cleaning up TUN and processes..."
ip link del dev {tun_if} 2>/dev/null || true
pkill -f "tun2socks --device {tun_if}" || true
if [ -f /tmp/tun2socks.pid ]; then
    kill $(cat /tmp/tun2socks.pid) 2>/dev/null || true
    rm -f /tmp/tun2socks.pid
fi

echo "Removing routing rules and iptables..."
ip rule del from {hotspot_subnet} table {table_id} 2>/dev/null || true
iptables -D FORWARD -i {tun_if} -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -o {tun_if} -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -i {wifi_if} -o {tun_if} -j ACCEPT 2>/dev/null || true
iptables -D FORWARD -i {tun_if} -o {wifi_if} -j ACCEPT 2>/dev/null || true

sysctl -w net.ipv4.conf.all.rp_filter=1 > /dev/null 2>&1 || true
sysctl -w net.ipv4.conf.{wifi_if}.rp_filter=1 > /dev/null 2>&1 || true

echo "Hotspot stopped and cleanup completed."
"#);

    run_privileged_script(&script)
}

