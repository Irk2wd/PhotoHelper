import { useState, useCallback, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./TransferView.css";

interface StartTransferResult {
  url: string;
  qr_svg: string;
}

interface FileReceivedEvent {
  name: string;
  size: number;
}

export default function TransferView() {
  const [folder, setFolder] = useState("");
  const [uploadFolder, setUploadFolder] = useState("");
  const [port, setPort] = useState(8765);
  const [running, setRunning] = useState(false);
  const [serverUrl, setServerUrl] = useState("");
  const [qrSvg, setQrSvg] = useState("");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [starting, setStarting] = useState(false);
  const [receivedFiles, setReceivedFiles] = useState<FileReceivedEvent[]>([]);

  // 初始化时检查服务是否已在运行
  useEffect(() => {
    invoke<boolean>("get_transfer_status").then((running) => {
      setRunning(running);
    });
    // 监听手机上传事件
    const unlisten = listen<FileReceivedEvent>("transfer://file-received", (event) => {
      setReceivedFiles((prev) => [event.payload, ...prev].slice(0, 100));
    });
    return () => {
      unlisten.then((fn) => fn());
    };
    // 组件卸载时不自动停止服务（允许后台持续运行）
  }, []);

  const pickFolder = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setFolder(selected);
    }
  }, []);

  const pickUploadFolder = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setUploadFolder(selected);
    }
  }, []);

  const handleStart = useCallback(async () => {
    if (!folder && !uploadFolder) {
      setError("请至少选择一个文件夹（照片文件夹或接收文件夹）");
      return;
    }
    setError("");
    setStarting(true);
    setReceivedFiles([]);
    try {
      const result = await invoke<StartTransferResult>("start_transfer", {
        folder,
        uploadFolder,
        port,
      });
      setServerUrl(result.url);
      setQrSvg(result.qr_svg);
      setRunning(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setStarting(false);
    }
  }, [folder, uploadFolder, port]);

  const handleStop = useCallback(async () => {
    await invoke("stop_transfer");
    setRunning(false);
    setServerUrl("");
    setQrSvg("");
  }, []);

  const handleCopy = useCallback(() => {
    if (!serverUrl) return;
    navigator.clipboard.writeText(serverUrl).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [serverUrl]);

  return (
    <div className="tv-container">
      <div className="tv-header">
        <h2 className="tv-title">传输到手机</h2>
        <p className="tv-desc">
          通过局域网 WiFi 将照片传输到手机。iPhone 用户首次需在 Safari 警告页点「高级 → 继续访问」，之后即可批量保存照片到相册。
        </p>
      </div>

      {/* 设置卡片 */}
      <div className="tv-card">
        {/* 文件夹选择 */}
        <div className="tv-row">
          <span className="tv-row-label">照片文件夹</span>
          <div className="tv-folder-group">
            <span className="tv-folder-path" title={folder}>
              {folder || "（未选择）"}
            </span>
            <button
              className="tv-btn-secondary"
              onClick={pickFolder}
              disabled={running}
            >
              选择…
            </button>
          </div>
        </div>

        <div className="tv-divider" />

        {/* 手机上传接收文件夹 */}
        <div className="tv-row">
          <span className="tv-row-label">接收文件夹</span>
          <div className="tv-folder-group">
            <span className="tv-folder-path" title={uploadFolder}>
              {uploadFolder || "（同照片文件夹）"}
            </span>
            <button
              className="tv-btn-secondary"
              onClick={pickUploadFolder}
              disabled={running}
            >
              选择…
            </button>
          </div>
        </div>

        <div className="tv-divider" />

        {/* 端口 */}
        <div className="tv-row">
          <span className="tv-row-label">服务端口</span>
          <input
            type="number"
            className="tv-port-input"
            value={port}
            min={1024}
            max={65535}
            onChange={(e) => setPort(Number(e.target.value))}
            disabled={running}
          />
        </div>
      </div>

      {/* 错误提示 */}
      {error && <div className="tv-error">{error}</div>}

      {/* 启动/停止按钮 */}
      <div className="tv-action-row">
        {running ? (
          <button className="tv-btn-stop" onClick={handleStop}>
            停止服务
          </button>
        ) : (
          <button
            className="tv-btn-start"
            onClick={handleStart}
            disabled={starting || (!folder && !uploadFolder)}
          >
            {starting ? "启动中…" : "启动传输服务"}
          </button>
        )}
        <span className={`tv-status-dot ${running ? "tv-status-dot--on" : ""}`} />
        <span className="tv-status-text">
          {running ? "服务运行中" : "服务已停止"}
        </span>
      </div>

      {/* 二维码 + 说明（服务运行时显示） */}
      {running && serverUrl && (
        <div className="tv-qr-card">
          <p className="tv-qr-hint">
            手机连接同一 WiFi 后，使用相机扫描下方二维码，或在浏览器输入地址：
          </p>
          <button
            className="tv-url-pill"
            onClick={handleCopy}
            title="点击复制"
          >
            {serverUrl}
            <span className="tv-url-copy-icon">
              {copied ? (
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              ) : (
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
                  <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
                </svg>
              )}
            </span>
          </button>
          <div
            className="tv-qr-wrapper"
            dangerouslySetInnerHTML={{ __html: qrSvg }}
          />
          <p className="tv-qr-tip">
                        iPhone：首次访问点「高级」→「继续访问」信任证书，大后即可批量保存：勾选 →「保存到相册」→「存储 X 张图像」。
          </p>
        </div>
      )}

      {/* 接收文件日志（手机上传到电脑） */}
      {running && receivedFiles.length > 0 && (
        <div className="tv-card tv-received-log">
          <div className="tv-received-title">已接收 {receivedFiles.length} 个文件</div>
          {receivedFiles.map((f, i) => (
            <div key={i} className="tv-received-item">
              <span className="tv-received-name">{f.name}</span>
              <span className="tv-received-size">{(f.size / 1024).toFixed(0)} KB</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
