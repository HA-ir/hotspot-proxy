# Hotspot Proxy

A cross-platform desktop application built with Tauri, React, and TypeScript that allows you to share your network connection through a WiFi hotspot while seamlessly routing all connected devices through a system-wide proxy (e.g., SOCKS5).

## Features

- **One-Click Hotspot:** Easily start and stop a secure WiFi hotspot.
- **Transparent Proxy Routing:** Automatically routes connected devices through your specified proxy (e.g., v2ray, shadowsocks).
- **Device Management:** Monitor connected devices (IP and MAC address) in real-time.
- **Live Logs:** Built-in console to view system logs and debug connection issues.

## Prerequisites

- Node.js
- Rust
- Cargo
- `hostapd` and `dnsmasq` (for Linux hotspot functionality)

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) 
- [Tauri Extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) 
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev
```

## Build

```bash
# Build the application for production
npm run tauri build
```
