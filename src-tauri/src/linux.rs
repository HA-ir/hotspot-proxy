use std::process::Command;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::{HashMap, HashSet};

fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn check_dependencies() -> Vec<crate::MissingDependency> {
    let mut missing = Vec::new();

    if !command_exists("nmcli") {
        missing.push(crate::MissingDependency {
            name: "networkmanager (nmcli)".to_string(),
            install_command: Some("sudo apt install network-manager".to_string()),
        });
    }

    if !command_exists("ip") {
        missing.push(crate::MissingDependency {
            name: "iproute2 (ip)".to_string(),
            install_command: Some("sudo apt install iproute2".to_string()),
        });
    }

    if !command_exists("iptables") {
        missing.push(crate::MissingDependency {
            name: "iptables".to_string(),
            install_command: Some("sudo apt install iptables".to_string()),
        });
    }

    if !command_exists("pkexec") {
        missing.push(crate::MissingDependency {
            name: "polkit (pkexec)".to_string(),
            install_command: Some("sudo apt install policykit-1".to_string()),
        });
    }

    missing
}

/// Auto-detect the first Wi-Fi capable network device via NetworkManager.
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

pub fn get_connected_devices() -> Result<Vec<crate::Device>, String> {
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
        match h.join() {
            Ok(Some((ip, mac))) => {
                let name = hostnames.get(&mac).cloned().unwrap_or_else(|| "Unknown Device".to_string());
                devices.push(crate::Device { ip, mac, name });
            }
            Ok(None) => {}
            Err(_) => eprintln!("ping thread panicked"),
        }
    }

    Ok(devices)
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

    let is_http = proxy_url.to_lowercase().starts_with("http");

    // DNS Handling Strategy:
    // - SOCKS5: We DNAT all DNS traffic from the hotspot to 8.8.8.8. It gets routed into tun0,
    //   and tun2socks natively proxies the UDP DNS to 8.8.8.8 over the SOCKS5 tunnel safely.
    // - HTTP: HTTP proxies don't support UDP. We bypass tun0 for all DNS queries (UDP 53)
    //   by routing them to the 'main' table, so the host resolves them directly.
    let dns_setup = if is_http {
        format!("ip rule add from {} ipproto udp dport 53 table main pref 90 2>/dev/null || true", hotspot_subnet)
    } else {
        format!("iptables -t nat -I PREROUTING -i {} -p udp --dport 53 -j DNAT --to-destination 8.8.8.8:53", wifi_if)
    };

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
fi
# Always update the SSID and Password in case they were changed in the UI
nmcli con modify "{hotspot_con_name}" 802-11-wireless.ssid {ssid_q} wifi-sec.key-mgmt wpa-psk wifi-sec.psk {password_q}
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

# Apply DNS strategy (SOCKS5 vs HTTP)
{dns_setup}

# Ensure local subnet traffic (like dnsmasq replies) bypasses the tunnel
ip rule add to {hotspot_subnet} table main pref 95 2>/dev/null || true

# Route everything else from the hotspot subnet into tun2socks
ip rule add from {hotspot_subnet} table {table_id} pref 100 2>/dev/null || true
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
    let wifi_if = detect_wifi_interface()?;
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
iptables -t nat -D PREROUTING -i {wifi_if} -p udp --dport 53 -j DNAT --to-destination 8.8.8.8:53 2>/dev/null || true
ip rule del from {hotspot_subnet} ipproto udp dport 53 table main pref 90 2>/dev/null || true
ip rule del to {hotspot_subnet} table main pref 95 2>/dev/null || true
ip rule del from {hotspot_subnet} table {table_id} pref 100 2>/dev/null || true
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
