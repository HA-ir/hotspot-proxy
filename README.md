# Hotspot Proxy

A desktop app that turns your Linux machine into a Wi-Fi hotspot and routes every connected device's traffic through a SOCKS5 proxy — no per-device configuration required.

Built with [Tauri 2](https://tauri.app), [React](https://react.dev), and Rust.

---

## How it works

1. Creates a Wi-Fi access point via NetworkManager (`nmcli`)
2. Brings up a TUN interface and starts [tun2socks](https://github.com/xjasonlyu/tun2socks) to forward all traffic through your SOCKS5 proxy
3. Adds routing rules and iptables so every device that joins the hotspot is transparently proxied — including DNS

Connected devices appear in real time. The app probes each ARP entry with a live ping so the device list is always accurate, not stale.

---

## Features

- One-click hotspot start/stop with pkexec authentication
- Transparent SOCKS5 routing for all connected devices (TCP + DNS)
- Real-time device list — connects and disconnects logged with timestamps
- Hostname resolution via DHCP lease enrichment
- Supports both Linux and Windows builds

---

## Requirements

**Linux**

| Dependency | Purpose |
|---|---|
| NetworkManager + nmcli | Create and manage the hotspot connection |
| pkexec | Run privileged network commands |
| iproute2 (`ip`) | TUN interface and routing table management |
| iptables | Forward traffic between hotspot and TUN interfaces |
| ping | Live device presence probing |

**SOCKS5 proxy** running locally or accessible from your machine (e.g. `socks5://127.0.0.1:10808`).

---

## Getting started

### Prerequisites

```bash
# Arch
sudo pacman -S networkmanager iproute2 iptables

# Ubuntu / Debian
sudo apt install network-manager iproute2 iptables
```

Install Rust and the Tauri CLI:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo install tauri-cli --version "^2"
```

Install Node dependencies:

```bash
npm install
```

### Development

```bash
npm run tauri dev
```

### Production build

```bash
npm run tauri build
```

The bundled app includes the `tun2socks` sidecar binary for both Linux (`x86_64`) and Windows (`x86_64`).

---

## Configuration

All settings are entered in the app UI before starting the hotspot:

| Field | Description | Default |
|---|---|---|
| Network Name (SSID) | The Wi-Fi name devices will see | `MySecureHotspot` |
| Password | WPA2 passphrase (8–63 chars) | — |
| Proxy URL | SOCKS5 endpoint to route traffic through | `socks5://127.0.0.1:10808` |

Settings are locked while the hotspot is active.

---

## How device detection works

Every 1.5 seconds the app:

1. Reads the ARP neighbor table (`ip neigh show dev <iface>`) for candidate IPs
2. Pings each candidate in parallel with a 500 ms timeout
3. Only devices that respond are shown as connected

This eliminates stale ARP entries that linger for minutes after a device disconnects, giving you an accurate live view.

---

## Tech stack

| Layer | Technology |
|---|---|
| UI | React 19 + TypeScript |
| Build tool | Vite 7 |
| Desktop shell | Tauri 2 |
| Backend | Rust |
| Traffic forwarding | tun2socks |
| Hotspot management | NetworkManager (nmcli) |

---

## Project structure

```
hotspot-proxy-app/
├── src/                  # React frontend
│   └── App.tsx           # Main UI — controls, device list, log console
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs        # Tauri commands: start/stop hotspot, device detection
│   │   └── linux.rs      # Linux-specific hotspot and routing logic
│   ├── bin/              # Bundled tun2socks binaries
│   └── tauri.conf.json   # App config (window size, bundle targets, sidecar)
└── package.json
```

---

## License

MIT
