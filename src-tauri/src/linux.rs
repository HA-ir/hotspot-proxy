use std::process::Command;
use std::fs;
use std::os::unix::fs::PermissionsExt;

pub fn start(_app: &tauri::AppHandle, ssid: &str, password: &str, proxy_url: &str) -> Result<String, String> {
    let wifi_if = "wlp0s20f3"; // Make this configurable later if needed
    let hotspot_con_name = "ProxyHotspot";
    let tun_if = "tun0";
    let tun_ip = "10.0.0.1";
    let hotspot_subnet = "10.42.0.0/24";
    let table_id = "100";

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

    let tun2socks_bin = tun2socks_path.to_str().unwrap();

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
    nmcli con add type wifi ifname {wifi_if} con-name "{hotspot_con_name}" autoconnect no ssid "{ssid}" > /dev/null
    nmcli con modify "{hotspot_con_name}" 802-11-wireless.mode ap 802-11-wireless.band bg ipv4.method shared ipv6.method disabled
    nmcli con modify "{hotspot_con_name}" wifi-sec.key-mgmt wpa-psk wifi-sec.psk "{password}"
fi
nmcli con up "{hotspot_con_name}" > /dev/null

echo "Setting up TUN interface..."
sysctl -w net.ipv4.ip_forward=1 > /dev/null
ip tuntap add mode tun dev {tun_if}
ip addr add {tun_ip}/24 dev {tun_if}
ip link set {tun_if} mtu 1500
ip link set {tun_if} up

echo "Starting tun2socks..."
{tun2socks_bin} --device {tun_if} --proxy "{proxy_url}" --loglevel debug > /var/log/tun2socks.log 2>&1 &
echo $! > /tmp/tun2socks.pid

echo "Applying routing and iptables..."
sysctl -w net.ipv4.conf.all.rp_filter=0 > /dev/null
sysctl -w net.ipv4.conf.{tun_if}.rp_filter=0 > /dev/null
sysctl -w net.ipv4.conf.{wifi_if}.rp_filter=0 > /dev/null

ip rule add from {hotspot_subnet} table {table_id} 2>/dev/null || true
ip route add default dev {tun_if} table {table_id} 2>/dev/null || true
ip route flush cache

iptables -I FORWARD -i {tun_if} -j ACCEPT
iptables -I FORWARD -o {tun_if} -j ACCEPT
iptables -I FORWARD -i {wifi_if} -o {tun_if} -j ACCEPT
iptables -I FORWARD -i {tun_if} -o {wifi_if} -j ACCEPT

# Fix for Browser/DNS (Force DNS queries through the tunnel)
iptables -t nat -I PREROUTING -i {wifi_if} -p udp --dport 53 -j DNAT --to-destination 8.8.8.8:53
iptables -t nat -I PREROUTING -i {wifi_if} -p tcp --dport 53 -j DNAT --to-destination 8.8.8.8:53

echo "✅ Hotspot '{ssid}' started successfully and routed through {proxy_url}"
"#);

    let script_path = "/tmp/hotspot_start.sh";
    fs::write(script_path, script).map_err(|e| format!("Failed to write start script: {}", e))?;
    let mut perms = fs::metadata(script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script_path, perms).unwrap();

    let output = Command::new("pkexec")
        .arg("bash")
        .arg(script_path)
        .output()
        .map_err(|e| format!("Failed to execute pkexec: {}", e))?;

    // Optionally remove the temp script
    let _ = fs::remove_file(script_path);

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.into_owned())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Exit Code: {:?}\n-- STDOUT --\n{}\n-- STDERR --\n{}", output.status.code(), stdout, stderr))
    }
}

pub fn stop() -> Result<String, String> {
    let wifi_if = "wlp0s20f3"; 
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

# Remove DNS DNAT rules
iptables -t nat -D PREROUTING -i {wifi_if} -p udp --dport 53 -j DNAT --to-destination 8.8.8.8:53 2>/dev/null || true
iptables -t nat -D PREROUTING -i {wifi_if} -p tcp --dport 53 -j DNAT --to-destination 8.8.8.8:53 2>/dev/null || true

sysctl -w net.ipv4.conf.all.rp_filter=1 > /dev/null 2>&1 || true
sysctl -w net.ipv4.conf.{wifi_if}.rp_filter=1 > /dev/null 2>&1 || true

echo "✅ Hotspot stopped and cleanup completed."
"#);

    let script_path = "/tmp/hotspot_stop.sh";
    fs::write(script_path, script).map_err(|e| format!("Failed to write stop script: {}", e))?;
    let mut perms = fs::metadata(script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(script_path, perms).unwrap();

    let output = Command::new("pkexec")
        .arg("bash")
        .arg(script_path)
        .output()
        .map_err(|e| format!("Failed to execute pkexec: {}", e))?;

    let _ = fs::remove_file(script_path);

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.into_owned())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Exit Code: {:?}\n-- STDOUT --\n{}\n-- STDERR --\n{}", output.status.code(), stdout, stderr))
    }
}