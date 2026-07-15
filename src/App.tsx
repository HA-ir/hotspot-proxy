import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface Device {
  ip: string;
  mac: string;
  name: string;
}

function App() {
  const [ssid, setSsid] = useState("MySecureHotspot");
  const [password, setPassword] = useState("SecurePassword123");
  const [proxyUrl, setProxyUrl] = useState("socks5://127.0.0.1:10808");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [isActive, setIsActive] = useState(false);
  const [devices, setDevices] = useState<Device[]>([]);
  const [toast, setToast] = useState("");
  
  const logEndRef = useRef<HTMLDivElement>(null);

  // Auto-scroll logs
  useEffect(() => {
    if (logEndRef.current) {
      logEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [status]);

  const showToast = (msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(""), 3000);
  };

  const addLog = (msg: string) => {
    const timestamp = new Date().toLocaleTimeString([], { hour12: false });
    setStatus(prev => prev ? `${prev}\n[${timestamp}] ${msg}` : `[${timestamp}] ${msg}`);
  };

  const clearLogs = () => setStatus("");
  
  const copyLogs = async () => {
    try {
      await navigator.clipboard.writeText(status);
      showToast("Logs copied to clipboard!");
    } catch (e) {
      console.error("Failed to copy", e);
      showToast("Failed to copy logs.");
    }
  };

  async function fetchDevices() {
    if (!isActive) return;
    try {
      const newDevs = await invoke<Device[]>("get_connected_devices");
      
      setDevices(prevDevs => {
        // Create sets of MAC addresses to avoid duplicate logs in React Strict Mode
        // and to handle proper diffing
        const prevMacs = new Set(prevDevs.map(d => d.mac));
        const newMacs = new Set(newDevs.map(d => d.mac));

        // Check for newly connected devices (in new, not in prev)
        newDevs.forEach(nd => {
          if (!prevMacs.has(nd.mac)) {
            addLog(`🟢 Device Connected: ${nd.ip} (MAC: ${nd.mac})`);
          }
        });
        
        // Check for disconnected devices (in prev, not in new)
        prevDevs.forEach(pd => {
          if (!newMacs.has(pd.mac)) {
            addLog(`🔴 Device Disconnected: ${pd.ip} (MAC: ${pd.mac})`);
          }
        });
        
        return newDevs;
      });
    } catch (e) {
      console.error(e);
    }
  }

  // Poll devices every 5 seconds
  useEffect(() => {
    let interval: number;
    if (isActive) {
      fetchDevices(); // immediate
      interval = window.setInterval(fetchDevices, 5000);
    } else {
      setDevices([]);
    }
    return () => clearInterval(interval);
  }, [isActive]);

  async function toggleHotspot() {
    setLoading(true);
    addLog(isActive ? "Initiating shutdown sequence..." : "Initiating hotspot sequence. Please authenticate if prompted...");
    try {
      if (isActive) {
        const res = await invoke("stop_hotspot");
        addLog(`✅ Hotspot Stopped!\n${res}`);
        setIsActive(false);
      } else {
        const res = await invoke("start_hotspot", { ssid, password, proxyUrl });
        addLog(`✅ Hotspot Started!\n${res}`);
        setIsActive(true);
      }
    } catch (error) {
      addLog(`❌ Error:\n${error}`);
      if (!isActive) setIsActive(false); 
    }
    setLoading(false);
  }

  return (
    <main className="container">
      {toast && <div className="toast">{toast}</div>}

      <header className="header">
        <div className="logo-container">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" className="wifi-icon">
            <path d="M5 12.55a11 11 0 0 1 14.08 0"></path>
            <path d="M1.42 9a16 16 0 0 1 21.16 0"></path>
            <path d="M8.53 16.11a6 6 0 0 1 6.95 0"></path>
            <line x1="12" y1="20" x2="12.01" y2="20"></line>
          </svg>
        </div>
        <div>
          <h1>Hotspot Proxy</h1>
          <p className="subtitle">Route all devices through SOCKS5</p>
        </div>
      </header>

      <div className="layout-grid">
        {/* Left Column: Controls */}
        <div className="controls-col">
          <div className="card shadow-sm">
            <h2 className="card-title">Network Configuration</h2>
            
            <div className="form-group">
              <label>Network Name (SSID)</label>
              <div className="input-wrapper">
                <input
                  type="text"
                  value={ssid}
                  onChange={(e) => setSsid(e.currentTarget.value)}
                  disabled={isActive || loading}
                  placeholder="e.g. MyWiFi"
                />
              </div>
            </div>

            <div className="form-group">
              <label>Password</label>
              <div className="input-wrapper">
                <input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.currentTarget.value)}
                  disabled={isActive || loading}
                  placeholder="Minimum 8 characters"
                />
              </div>
            </div>

            <div className="form-group">
              <label>Proxy URL</label>
              <div className="input-wrapper">
                <input
                  type="text"
                  value={proxyUrl}
                  onChange={(e) => setProxyUrl(e.currentTarget.value)}
                  disabled={isActive || loading}
                  placeholder="socks5://127.0.0.1:10808"
                />
              </div>
            </div>

            <button 
              onClick={toggleHotspot} 
              disabled={loading} 
              className={`toggle-btn ${isActive ? 'active' : ''} ${loading ? 'loading' : ''}`}
            >
              {loading ? (
                <span className="spinner"></span>
              ) : isActive ? (
                <>
                  <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="2.5"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect></svg>
                  Stop Hotspot
                </>
              ) : (
                <>
                  <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="2.5"><polygon points="5 3 19 12 5 21 5 3"></polygon></svg>
                  Start Hotspot
                </>
              )}
            </button>
          </div>

          <div className={`card shadow-sm devices-card ${isActive ? 'visible' : 'hidden'}`}>
            <div className="card-header">
              <h2 className="card-title">Connected Devices</h2>
              <span className="badge">{devices.length}</span>
            </div>
            
            {devices.length === 0 ? (
              <p className="no-devices">No devices connected yet.</p>
            ) : (
              <ul className="device-list">
                {devices.map((d, i) => (
                  <li key={i} className="device-item">
                    <div className="device-icon">
                      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2"><rect x="4" y="4" width="16" height="16" rx="2" ry="2"></rect><rect x="9" y="9" width="6" height="6"></rect><line x1="9" y1="1" x2="9" y2="4"></line><line x1="15" y1="1" x2="15" y2="4"></line><line x1="9" y1="20" x2="9" y2="23"></line><line x1="15" y1="20" x2="15" y2="23"></line><line x1="20" y1="9" x2="23" y2="9"></line><line x1="20" y1="14" x2="23" y2="14"></line><line x1="1" y1="9" x2="4" y2="9"></line><line x1="1" y1="14" x2="4" y2="14"></line></svg>
                    </div>
                    <div className="device-info">
                      <strong>{d.ip}</strong>
                      <span>{d.mac}</span>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>

        {/* Right Column: Logs */}
        <div className="logs-col">
          <div className="card shadow-sm logs-card">
            <div className="status-header">
              <div className="status-left">
                <span className={`status-indicator ${isActive ? 'online' : 'offline'}`}></span>
                <h2 className="card-title">Console Log</h2>
              </div>
              <div className="log-actions">
                <button onClick={copyLogs} className="icon-btn" title="Copy Logs">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
                </button>
                <button onClick={clearLogs} className="icon-btn danger" title="Clear Logs">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
                </button>
              </div>
            </div>
            <div className="log-scroll-area">
              <pre className="log-pre">{status || "System ready. Awaiting commands..."}</pre>
              <div ref={logEndRef} />
            </div>
          </div>
        </div>
      </div>
    </main>
  );
}

export default App;