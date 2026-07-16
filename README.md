# Hotspot Proxy

A lightweight desktop application that turns your machine into a Wi-Fi hotspot and transparently routes all connected clients' traffic through a SOCKS5 or HTTP proxy. No per-device configuration is required — clients connect to the Wi-Fi and are instantly proxied.

Built with [Tauri 2](https://tauri.app), [React](https://react.dev), and Rust.

## Features

- **Transparent Proxying:** All TCP, UDP, and DNS traffic from connected devices is routed through the proxy tunnel automatically.
- **Cross-Platform Support:** Works natively on both Linux and Windows 10/11.
- **Real-Time Device Tracking:** Active ARP probing ensures the connected devices list is accurate and never stale.
- **Zero Client Config:** Devices connect to the hotspot with a standard WPA2 password; routing happens seamlessly at the network layer on the host machine.
- **Self-Contained:** Bundles the high-performance `tun2socks` routing engine.

---

## How it works

### Linux
1. Creates a WPA2 Wi-Fi access point via NetworkManager (`nmcli`).
2. Provisions a `tun0` interface and launches `tun2socks` attached to your upstream proxy.
3. Applies `iptables` rules to bridge traffic from the hotspot interface into the TUN interface.

### Windows
1. Provisions a Mobile Hotspot via the native WinRT `NetworkOperatorTetheringManager` API (bypassing legacy `netsh` limitations).
2. Launches `tun2socks` with the WinTUN driver to create a virtual network adapter.
3. Adjusts the Windows routing table and enables IP forwarding (`IPEnableRouter`) to funnel ICS (Internet Connection Sharing) traffic through the WinTUN adapter.

---

## Prerequisites

### Linux
The following standard network utilities must be installed on your system:
```bash
# Arch Linux
sudo pacman -S networkmanager iproute2 iptables

# Ubuntu / Debian
sudo apt install network-manager iproute2 iptables
```

### Windows
Windows requires the `wintun.dll` kernel driver to establish the virtual network interface.
When you launch the app on Windows for the first time, it will prompt you to automatically download `wintun.dll` from the official source (`wintun.net`).

---

## Getting Started

1. **Install Build Dependencies:**
   Ensure you have Node.js and Rust installed.
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   npm install
   ```

2. **Run in Development Mode:**
   ```bash
   npm run tauri dev
   ```

3. **Build for Production:**
   ```bash
   npm run tauri build
   ```
   The bundled executable will be generated in `src-tauri/target/release/bundle/`.

---

## Configuration

All configuration is done directly in the UI before initiating the hotspot:

| Field | Description | Default |
|---|---|---|
| **Network Name (SSID)** | The Wi-Fi name broadcasted to devices. | `MySecureHotspot` |
| **Password** | WPA2 security passphrase (min. 8 characters). | — |
| **Proxy URL** | The upstream SOCKS5 or HTTP endpoint. | `socks5://127.0.0.1:10808` |

> **⚠️ HTTP Proxy DNS Limitation:** Standard HTTP proxies do not support UDP forwarding. If you use an `http://` proxy, the app automatically bypasses the proxy tunnel for DNS queries so your browser continues to work. This means DNS requests will be visible to your local ISP. **For maximum privacy, always use `socks5://`.**

---

## Device Detection Mechanism

Most hotspots suffer from "ghost devices" — devices that remain in the ARP table for minutes after disconnecting. Hotspot Proxy solves this by:
1. Reading the host's ARP neighbor table to find candidate IPs.
2. Pinging each candidate in parallel with an aggressive 500ms timeout.
3. Filtering the display to show *only* devices that actively respond to ICMP echoes.

---

## Tech Stack

- **UI:** React 19 + TypeScript
- **Styling:** Custom CSS (No heavy UI frameworks)
- **Desktop Shell:** Tauri 2
- **Systems Integration:** Rust
- **Core Routing:** `tun2socks` (Go)

---

## License

MIT
