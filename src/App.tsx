import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./App.css";

interface Device {
  ip: string;
  mac: string;
  name: string;
}

interface MissingDependency {
  name: string;
  install_command: string | null;
}

type SysCheckState = "checking" | "ok" | "error";

function SystemCheckModal({ onDone }: { onDone: () => void }) {
  const [state, setState] = useState<SysCheckState>("checking");
  const [missing, setMissing] = useState<MissingDependency[]>([]);
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  useEffect(() => {
    invoke<MissingDependency[]>("check_system_dependencies")
      .then(deps => {
        if (deps.length === 0) {
          setState("ok");
          onDoneRef.current();
        } else {
          setMissing(deps);
          setState("error");
        }
      })
      .catch((err) => {
        console.error("System dependency check failed:", err);
        setState("ok");
        onDoneRef.current();
      });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (state === "checking" || state === "ok") return null;

  return (
    <div className="modal-backdrop" style={{ zIndex: 3000 }}>
      <div className="modal-card">
        <h2 className="modal-title">Missing System Dependencies</h2>
        <p className="modal-body">
          This app requires certain core networking tools to function. The following tools were not found in your system's PATH:
        </p>

        <ul className="modal-body" style={{ background: "#f8f9fa", padding: "10px 10px 10px 25px", borderRadius: "6px", fontFamily: "monospace", fontSize: "0.85rem" }}>
          {missing.map((dep, i) => (
            <li key={i} style={{ marginBottom: "8px" }}>
              <strong>{dep.name}</strong>
              {dep.install_command && (
                <div style={{ color: "var(--text-muted)", marginTop: "4px" }}>
                  <em>e.g. <code>{dep.install_command}</code></em>
                </div>
              )}
            </li>
          ))}
        </ul>

        <p className="modal-body" style={{ fontSize: "0.85rem", color: "var(--text-muted)" }}>
          Please install these tools and restart the application.
        </p>
      </div>
    </div>
  );
}

type WintunState = "checking" | "ok" | "prompt" | "downloading" | "error";

function WintunModal({ onDone }: { onDone: () => void }) {
  const [state, setState] = useState<WintunState>("checking");
  const [error, setError] = useState("");
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;

  useEffect(() => {
    invoke<boolean>("check_wintun")
      .then(ok => {
        setState(ok ? "ok" : "prompt");
        if (ok) onDoneRef.current();
      })
      .catch((err) => {
        console.error("wintun check failed:", err);
        // If the check command fails entirely (e.g. not registered), just let them through
        // so the app isn't bricked.
        setState("ok");
        onDoneRef.current();
      });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleDownload() {
    setState("downloading");
    try {
      await invoke("download_wintun");
      setState("ok");
      onDoneRef.current();
    } catch (e) {
      setError(String(e));
      setState("error");
    }
  }

  if (state === "checking" || state === "ok") return null;

  return (
    <div className="modal-backdrop">
      <div className="modal-card">
        <h2 className="modal-title">Required Component Missing</h2>
        {state === "prompt" && (
          <>
            <p className="modal-body">
              <strong>wintun.dll</strong> is required for proxy tunnelling on Windows but was not found.
              May the app download it automatically from <em>wintun.net</em>?
            </p>
            <div className="modal-actions">
              <button className="toggle-btn" onClick={handleDownload}>Download (≈ 500 KB)</button>
            </div>
          </>
        )}
        {state === "downloading" && (
          <p className="modal-body">
            <span className="spinner" style={{ display: "inline-block", marginRight: 10 }} />
            Downloading wintun.dll…
          </p>
        )}
        {state === "error" && (
          <>
            <p className="modal-body" style={{ color: "var(--danger)" }}>
              Download failed: {error}
            </p>
            <p className="modal-body" style={{ fontSize: "0.82rem" }}>
              You can place <strong>wintun.dll</strong> manually next to the app executable and restart.
            </p>
            <div className="modal-actions">
              <button className="toggle-btn" onClick={handleDownload}>Retry</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function App() {
  const [ssid, setSsid] = useState("MySecureHotspot");
  const [password, setPassword] = useState("SecurePassword123");
  const [showPassword, setShowPassword] = useState(false);
  const [proxyUrl, setProxyUrl] = useState("socks5://127.0.0.1:10808");
  const [status, setStatus] = useState("");
  const [loading, setLoading] = useState(false);
  const [isActive, setIsActive] = useState(false);
  const [devices, setDevices] = useState<Device[]>([]);
  const [toast, setToast] = useState("");
  const [wintunReady, setWintunReady] = useState(false);
  const [sysReady, setSysReady] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);

  const logEndRef = useRef<HTMLDivElement>(null);
  const logContainerRef = useRef<HTMLPreElement>(null);

  const isSsidValid = ssid.trim().length > 0 && ssid.length <= 32;
  const isPasswordValid = password.length >= 8 && password.length <= 63;
  const canStart = isSsidValid && isPasswordValid;

  // Auto-scroll logs
  useEffect(() => {
    if (autoScroll && logContainerRef.current) {
      // Use instant scroll. Smooth scrolling can trigger the onScroll handler
      // mid-animation and accidentally disable auto-scroll.
      logContainerRef.current.scrollTop = logContainerRef.current.scrollHeight;
    }
  }, [status, autoScroll]);

  // Detect manual scrolling up
  const handleLogScroll = () => {
    if (!logContainerRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = logContainerRef.current;

    // Increased threshold to 30px to account for browser sub-pixel rounding
    const isAtBottom = Math.abs(scrollHeight - clientHeight - scrollTop) <= 30;

    if (!isAtBottom && autoScroll) {
      setAutoScroll(false);
    } else if (isAtBottom && !autoScroll) {
      setAutoScroll(true);
    }
  };

  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    if (toastTimerRef.current !== null) clearTimeout(toastTimerRef.current);
    toastTimerRef.current = setTimeout(() => {
      setToast("");
      toastTimerRef.current = null;
    }, 3000);
  }, []);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current !== null) clearTimeout(toastTimerRef.current);
    };
  }, []);

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

  const devicesRef = useRef<Device[]>([]);
  const isFetchingRef = useRef(false);
  const isActiveRef = useRef(false);

  useEffect(() => {
    isActiveRef.current = isActive;
  }, [isActive]);

  const addLogRef = useRef(addLog);
  useEffect(() => {
    addLogRef.current = addLog;
  });

  // Diff newDevs against devicesRef and emit connect/disconnect logs, then commit.
  function applyDeviceDiff(newDevs: Device[]) {
    const prevDevs = devicesRef.current;
    const prevMacs = new Set(prevDevs.map(d => d.mac));
    const newMacs = new Set(newDevs.map(d => d.mac));

    newDevs.forEach(nd => {
      if (!prevMacs.has(nd.mac)) {
        addLogRef.current(`🟢 Device Connected: ${nd.ip} (MAC: ${nd.mac})`);
      }
    });

    prevDevs.forEach(pd => {
      if (!newMacs.has(pd.mac)) {
        addLogRef.current(`🔴 Device Disconnected: ${pd.ip} (MAC: ${pd.mac})`);
      }
    });

    devicesRef.current = newDevs;
    setDevices(newDevs);
  }

  async function fetchDevices() {
    if (!isActiveRef.current || isFetchingRef.current) return;
    isFetchingRef.current = true;
    try {
      const newDevs = await invoke<Device[]>("get_connected_devices");
      applyDeviceDiff(newDevs);
    } catch (e) {
      console.error(e);
    } finally {
      isFetchingRef.current = false;
    }
  }

  // Poll devices every 2 seconds while hotspot is active
  useEffect(() => {
    let interval: number;
    if (isActive) {
      devicesRef.current = [];
      fetchDevices();
      interval = window.setInterval(fetchDevices, 1500);
    }
    // Cleanup on stop: interval cleared, devicesRef already zeroed by stop handler
    return () => clearInterval(interval);
  }, [isActive]);

  async function toggleHotspot() {
    if (!isActive && !canStart) {
      addLog("❌ Fix the highlighted fields before starting the hotspot.");
      return;
    }

    setLoading(true);
    addLog(isActive ? "Initiating shutdown sequence..." : "Initiating hotspot sequence. Authenticate if prompted.");
    try {
      if (isActive) {
        const res = await invoke("stop_hotspot");
        // Emit disconnects for all devices still tracked, then clear immediately
        applyDeviceDiff([]);
        addLog(`✅ Hotspot Stopped!\n${res}`);
        setIsActive(false);
      } else {
        const res = await invoke("start_hotspot", { ssid, password, proxyUrl });
        addLog(`✅ Hotspot Started!\n${res}`);
        setIsActive(true);
      }
    } catch (error) {
      addLog(`❌ Error:\n${error}`);
    }
    setLoading(false);
  }

  return (
    <>
      {!sysReady && <SystemCheckModal onDone={() => setSysReady(true)} />}
      {sysReady && !wintunReady && <WintunModal onDone={() => setWintunReady(true)} />}
      <main className="container">
        {toast && <div className="toast">{toast}</div>}

      <header className="header">
        <div className="header-left">
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
            <p className="subtitle">Route all devices through a proxy</p>
          </div>
        </div>
        <button
          onClick={(e) => {
            e.preventDefault();
            openUrl("https://github.com/HA-ir/hotspot-proxy");
          }}
          className="github-link"
          title="View on GitHub"
        >
          <svg viewBox="0 0 24 24" width="20" height="20" fill="currentColor">
            <path d="M12 2C6.477 2 2 6.477 2 12c0 4.42 2.865 8.166 6.839 9.489.5.092.682-.217.682-.482 0-.237-.008-.866-.013-1.7-2.782.603-3.369-1.34-3.369-1.34-.454-1.156-1.11-1.462-1.11-1.462-.908-.62.069-.608.069-.608 1.003.07 1.531 1.03 1.531 1.03.892 1.529 2.341 1.087 2.91.831.092-.646.35-1.086.636-1.336-2.22-.253-4.555-1.11-4.555-4.943 0-1.091.39-1.984 1.029-2.683-.103-.253-.446-1.27.098-2.647 0 0 .84-.269 2.75 1.025A9.578 9.578 0 0112 6.836c.85.004 1.705.114 2.504.336 1.909-1.294 2.747-1.025 2.747-1.025.546 1.379.203 2.394.1 2.647.64.699 1.028 1.592 1.028 2.683 0 3.842-2.339 4.687-4.566 4.935.359.309.678.919.678 1.852 0 1.336-.012 2.415-.012 2.743 0 .267.18.578.688.48C19.138 20.161 22 16.416 22 12c0-5.523-4.477-10-10-10z"></path>
          </svg>
        </button>
      </header>

      <div className="layout-grid">
        {/* Controls */}
        <div className="controls-col">
          <div className="card shadow-sm">
            <h2 className="card-title">Network Configuration</h2>

            <div className="form-group">
              <label>
                Network Name (SSID)
                {!isSsidValid && ssid.length > 0 && <span className="field-hint invalid">Max 32 characters</span>}
              </label>
              <div className="input-wrapper">
                <input
                  type="text"
                  value={ssid}
                  onChange={(e) => setSsid(e.currentTarget.value)}
                  disabled={isActive || loading}
                  placeholder="e.g. MyWiFi"
                  className={!isSsidValid && ssid.length > 0 ? "invalid" : ""}
                  maxLength={32}
                />
              </div>
            </div>

            <div className="form-group">
              <label>
                Password
                <span className={`field-hint ${!isPasswordValid ? "invalid" : ""}`}>
                  {password.length}/63 (min 8)
                </span>
              </label>
              <div className="input-wrapper has-icon">
                <input
                  type={showPassword ? "text" : "password"}
                  value={password}
                  onChange={(e) => setPassword(e.currentTarget.value)}
                  disabled={isActive || loading}
                  placeholder="Minimum 8 characters"
                  className={!isPasswordValid ? "invalid" : ""}
                  maxLength={63}
                />
                <button
                  type="button"
                  className="eye-btn"
                  onClick={() => setShowPassword(!showPassword)}
                  title={showPassword ? "Hide password" : "Show password"}
                  disabled={isActive || loading}
                >
                  {showPassword ? (
                    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"></path>
                      <line x1="1" y1="1" x2="23" y2="23"></line>
                    </svg>
                  ) : (
                    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"></path>
                      <circle cx="12" cy="12" r="3"></circle>
                    </svg>
                  )}
                </button>
              </div>
            </div>

            <div className="form-group">
              <label>
                Proxy URL
                <span className="field-hint">SOCKS5 or HTTP</span>
              </label>
              <div className="input-wrapper">
                <input
                  type="text"
                  value={proxyUrl}
                  onChange={(e) => setProxyUrl(e.currentTarget.value)}
                  disabled={isActive || loading}
                  placeholder="socks5://... or http://..."
                />
              </div>
            </div>

            <button
              onClick={toggleHotspot}
              disabled={loading || (!isActive && !canStart)}
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

          <div className="card shadow-sm devices-card">
            <div className="card-header">
              <h2 className="card-title">Connected Devices</h2>
              <span className="badge">{devices.length}</span>
            </div>

            {devices.length === 0 ? (
              <p className="no-devices">No devices connected yet.</p>
            ) : (
              <ul className="device-list">
                {devices.map((d) => (
                  <li key={d.mac} className="device-item">
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

        {/* Logs */}
        <div className="logs-col">
          <div className="card shadow-sm logs-card">
            <div className="status-header">
              <div className="status-left">
                <span className={`status-indicator ${isActive ? 'online' : 'offline'}`}></span>
                <h2 className="card-title">Console Log</h2>
              </div>
              <div className="log-actions">
                <button
                  onClick={() => setAutoScroll(!autoScroll)}
                  className={`icon-btn ${autoScroll ? "active" : ""}`}
                  title={autoScroll ? "Disable Auto-scroll" : "Enable Auto-scroll"}
                >
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <line x1="12" y1="5" x2="12" y2="19"></line>
                    <polyline points="19 12 12 19 5 12"></polyline>
                  </svg>
                </button>
                <button onClick={copyLogs} className="icon-btn" title="Copy Logs">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path></svg>
                </button>
                <button onClick={clearLogs} className="icon-btn danger" title="Clear Logs">
                  <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6"></polyline><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path></svg>
                </button>
              </div>
            </div>
            <div className="log-scroll-area">
              <pre className="log-pre" ref={logContainerRef} onScroll={handleLogScroll}>
                {status || "System ready. Awaiting commands..."}
                <div ref={logEndRef} style={{ height: 1 }} />
              </pre>
            </div>
          </div>
        </div>
      </div>
    </main>
    </>
  );
}

export default App;