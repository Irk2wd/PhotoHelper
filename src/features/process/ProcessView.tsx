import { useState, useMemo } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import "./ProcessView.css";

// ─── 类型定义 ─────────────────────────────────────────────

interface ProcessFileInfo {
  name: string;
  size_bytes: number;
  compress_supported: boolean;
}

interface RenameConfig {
  pattern: "prefix_seq" | "keep_suffix" | "custom_seq";
  prefix: string;
  suffix: string;
  start_num: number;
  pad_digits: number;
}

interface CompressConfig {
  jpeg_quality: number;
  png_mode: "lossless" | "to_jpeg";
  png_jpeg_quality: number;
}

interface ExecuteProcessResult {
  processed: number;
  skipped_compress: number;
  failed: string[];
  output_folder: string;
}

// ─── 工具函数 ─────────────────────────────────────────────

function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

/** 前端实时计算输出文件名（与后端 compute_output_name 逻辑一致） */
function computeOutputName(
  original: string,
  index: number,
  rename: RenameConfig | null,
  compress: CompressConfig | null
): string {
  const dotIdx = original.lastIndexOf(".");
  const stem = dotIdx >= 0 ? original.slice(0, dotIdx) : original;
  const extRaw = dotIdx >= 0 ? original.slice(dotIdx).toLowerCase() : "";

  const outExt =
    compress && extRaw === ".png" && compress.png_mode === "to_jpeg"
      ? ".jpg"
      : extRaw;

  let outStem = stem;
  if (rename) {
    const num = String(rename.start_num + index).padStart(rename.pad_digits, "0");
    switch (rename.pattern) {
      case "prefix_seq":
        outStem = `${rename.prefix}${num}`;
        break;
      case "keep_suffix":
        outStem = `${stem}${rename.suffix}`;
        break;
      case "custom_seq":
        outStem = `${rename.prefix}${num}${rename.suffix}`;
        break;
    }
  }
  return `${outStem}${outExt}`;
}

// ─── 子组件：模块卡片 ─────────────────────────────────────

interface ModuleCardProps {
  title: string;
  enabled: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}

function ModuleCard({ title, enabled, onToggle, children }: ModuleCardProps) {
  return (
    <div className={`pv-module-card ${enabled ? "pv-module-card--on" : ""}`}>
      <div className="pv-module-header">
        <span className="pv-module-title">{title}</span>
        <button
          className={`pv-toggle-btn ${enabled ? "pv-toggle-btn--on" : ""}`}
          onClick={onToggle}
          aria-pressed={enabled}
        >
          <span className="pv-toggle-knob" />
        </button>
      </div>
      {enabled && <div className="pv-module-body">{children}</div>}
    </div>
  );
}

// ─── 主视图 ───────────────────────────────────────────────

type Phase = "idle" | "scanning" | "scanned" | "executing" | "done";

const DEFAULT_RENAME: RenameConfig = {
  pattern: "prefix_seq",
  prefix: "photo_",
  suffix: "",
  start_num: 1,
  pad_digits: 3,
};

const DEFAULT_COMPRESS: CompressConfig = {
  jpeg_quality: 85,
  png_mode: "lossless",
  png_jpeg_quality: 85,
};

export default function ProcessView() {
  const [folder, setFolder] = useState("");
  const [outputSubdir, setOutputSubdir] = useState("processed");
  const [phase, setPhase] = useState<Phase>("idle");
  const [files, setFiles] = useState<ProcessFileInfo[]>([]);
  const [result, setResult] = useState<ExecuteProcessResult | null>(null);
  const [error, setError] = useState("");

  // 模块开关
  const [renameEnabled, setRenameEnabled] = useState(false);
  const [compressEnabled, setCompressEnabled] = useState(true);

  // 模块配置
  const [rename, setRename] = useState<RenameConfig>({ ...DEFAULT_RENAME });
  const [compress, setCompress] = useState<CompressConfig>({ ...DEFAULT_COMPRESS });

  const isLoading = phase === "scanning" || phase === "executing";

  // 实时计算预览文件名列表
  const previewNames = useMemo(
    () =>
      files.map((f, i) =>
        computeOutputName(
          f.name,
          i,
          renameEnabled ? rename : null,
          compressEnabled ? compress : null
        )
      ),
    [files, renameEnabled, rename, compressEnabled, compress]
  );

  function resetState() {
    setPhase("idle");
    setFiles([]);
    setResult(null);
    setError("");
  }

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      setFolder(selected);
      resetState();
    }
  }

  async function handleScan() {
    if (!folder) return;
    setPhase("scanning");
    setError("");
    try {
      const data = await invoke<ProcessFileInfo[]>("scan_process", { folder });
      setFiles(data);
      setPhase("scanned");
    } catch (e) {
      setError(String(e));
      setPhase("idle");
    }
  }

  async function handleExecute() {
    setPhase("executing");
    setError("");
    try {
      const data = await invoke<ExecuteProcessResult>("execute_process", {
        args: {
          folder,
          output_subdir: outputSubdir || "processed",
          rename: renameEnabled ? rename : null,
          compress: compressEnabled ? compress : null,
        },
      });
      setResult(data);
      setPhase("done");
    } catch (e) {
      setError(String(e));
      setPhase("scanned");
    }
  }

  return (
    <div className="pv-root">
      {/* 标题 */}
      <div className="pv-header">
        <h1 className="pv-title">图片处理</h1>
        <p className="pv-subtitle">
          对文件夹内的图片批量执行模块化处理，可按需开启重命名、压缩等步骤，输出到独立子文件夹，不影响原文件
        </p>
      </div>

      {/* 文件夹选择 */}
      <div className="pv-card">
        <div className="pv-folder-row">
          <input
            className="pv-input"
            value={folder}
            onChange={(e) => { setFolder(e.target.value); resetState(); }}
            placeholder="选择或粘贴图片所在文件夹..."
          />
          <button className="sv-btn sv-btn--ghost" onClick={pickFolder} disabled={isLoading}>
            浏览...
          </button>
        </div>
        <div className="pv-output-row">
          <span className="pv-output-label">输出到子文件夹：</span>
          <input
            className="pv-input pv-input--sm"
            value={outputSubdir}
            onChange={(e) => setOutputSubdir(e.target.value)}
            placeholder="processed"
          />
        </div>
      </div>

      {/* 模块：重命名 */}
      <ModuleCard
        title="重命名"
        enabled={renameEnabled}
        onToggle={() => setRenameEnabled((v) => !v)}
      >
        <div className="pv-field-group">
          <label className="pv-field-label">命名模式</label>
          <div className="pv-radio-group">
            {(
              [
                ["prefix_seq", "前缀 + 序号"],
                ["keep_suffix", "保持原名 + 后缀"],
                ["custom_seq", "前缀 + 序号 + 后缀"],
              ] as [RenameConfig["pattern"], string][]
            ).map(([val, label]) => (
              <label key={val} className="pv-radio-item">
                <input
                  type="radio"
                  name="rename-pattern"
                  checked={rename.pattern === val}
                  onChange={() => setRename((r) => ({ ...r, pattern: val }))}
                />
                <span>{label}</span>
              </label>
            ))}
          </div>
        </div>

        <div className="pv-inline-fields">
          {rename.pattern !== "keep_suffix" && (
            <div className="pv-field-group">
              <label className="pv-field-label">前缀</label>
              <input
                className="pv-input pv-input--sm"
                value={rename.prefix}
                onChange={(e) => setRename((r) => ({ ...r, prefix: e.target.value }))}
                placeholder="photo_"
              />
            </div>
          )}
          {rename.pattern !== "prefix_seq" && (
            <div className="pv-field-group">
              <label className="pv-field-label">后缀</label>
              <input
                className="pv-input pv-input--sm"
                value={rename.suffix}
                onChange={(e) => setRename((r) => ({ ...r, suffix: e.target.value }))}
                placeholder="_edit"
              />
            </div>
          )}
          {rename.pattern !== "keep_suffix" && (
            <>
              <div className="pv-field-group">
                <label className="pv-field-label">起始编号</label>
                <input
                  className="pv-input pv-input--sm"
                  type="number"
                  min={0}
                  value={rename.start_num}
                  onChange={(e) =>
                    setRename((r) => ({ ...r, start_num: Math.max(0, parseInt(e.target.value) || 0) }))
                  }
                />
              </div>
              <div className="pv-field-group">
                <label className="pv-field-label">补零位数</label>
                <input
                  className="pv-input pv-input--sm"
                  type="number"
                  min={1}
                  max={9}
                  value={rename.pad_digits}
                  onChange={(e) =>
                    setRename((r) => ({
                      ...r,
                      pad_digits: Math.min(9, Math.max(1, parseInt(e.target.value) || 3)),
                    }))
                  }
                />
              </div>
            </>
          )}
        </div>

        {files.length > 0 && (
          <div className="pv-rename-preview">
            <span className="pv-field-label">预览：</span>
            <span className="pv-rename-preview-names">
              {previewNames.slice(0, 3).join("  ·  ")}
              {previewNames.length > 3 && "  ..."}
            </span>
          </div>
        )}
      </ModuleCard>

      {/* 模块：压缩 */}
      <ModuleCard
        title="压缩"
        enabled={compressEnabled}
        onToggle={() => setCompressEnabled((v) => !v)}
      >
        <div className="pv-field-group">
          <label className="pv-field-label">
            JPEG 质量：<strong>{compress.jpeg_quality}</strong>
          </label>
          <input
            type="range"
            min={1}
            max={100}
            value={compress.jpeg_quality}
            onChange={(e) =>
              setCompress((c) => ({ ...c, jpeg_quality: parseInt(e.target.value) }))
            }
            className="pv-slider"
          />
          <div className="pv-slider-hints">
            <span>小文件（低质量）</span><span>高质量（大文件）</span>
          </div>
        </div>

        <div className="pv-field-group">
          <label className="pv-field-label">PNG 处理方式</label>
          <div className="pv-radio-group">
            <label className="pv-radio-item">
              <input
                type="radio"
                name="png-mode"
                checked={compress.png_mode === "lossless"}
                onChange={() => setCompress((c) => ({ ...c, png_mode: "lossless" }))}
              />
              <span>保持 PNG 格式（无损）</span>
            </label>
            <label className="pv-radio-item">
              <input
                type="radio"
                name="png-mode"
                checked={compress.png_mode === "to_jpeg"}
                onChange={() => setCompress((c) => ({ ...c, png_mode: "to_jpeg" }))}
              />
              <span>转换为 JPEG</span>
            </label>
          </div>
          {compress.png_mode === "to_jpeg" && (
            <div className="pv-field-group" style={{ marginTop: 8 }}>
              <label className="pv-field-label">
                PNG → JPEG 质量：<strong>{compress.png_jpeg_quality}</strong>
              </label>
              <input
                type="range"
                min={1}
                max={100}
                value={compress.png_jpeg_quality}
                onChange={(e) =>
                  setCompress((c) => ({ ...c, png_jpeg_quality: parseInt(e.target.value) }))
                }
                className="pv-slider"
              />
            </div>
          )}
        </div>

        <div className="pv-compress-note">
          HEIF / HIF / AVIF 格式暂不支持压缩，将仅做重命名处理
        </div>
      </ModuleCard>

      {/* 扫描 & 执行按钮 */}
      <div className="pv-card">
        {error && <div className="sv-error">{error}</div>}
        <div className="pv-actions">
          <button
            className="sv-btn sv-btn--ghost"
            disabled={!folder || isLoading}
            onClick={handleScan}
          >
            {phase === "scanning" ? "扫描中..." : "扫描预览"}
          </button>
          {(phase === "scanned" || phase === "executing") && (
            <button
              className="sv-btn sv-btn--primary"
              disabled={files.length === 0 || isLoading || (!renameEnabled && !compressEnabled)}
              onClick={handleExecute}
            >
              {phase === "executing"
                ? "处理中..."
                : `执行处理（${files.length} 个文件）`}
            </button>
          )}
        </div>

        {!renameEnabled && !compressEnabled && phase === "scanned" && (
          <div className="pv-warn">请至少启用一个处理模块</div>
        )}
      </div>

      {/* 预览表格 */}
      {(phase === "scanned" || phase === "executing") && files.length > 0 && (
        <div className="pv-card">
          <div className="pv-section-label">
            共 {files.length} 个文件 · 输出到 {outputSubdir || "processed"}/
          </div>
          <div className="pv-table">
            <div className="pv-table-head">
              <span>原文件名</span>
              <span>大小</span>
              <span>输出文件名</span>
              <span>备注</span>
            </div>
            {files.map((f, i) => (
              <div key={f.name} className="pv-table-row">
                <span className="pv-cell-mono pv-cell-ellipsis" title={f.name}>{f.name}</span>
                <span className="pv-cell-size">{fmtSize(f.size_bytes)}</span>
                <span className="pv-cell-mono pv-cell-ellipsis" title={previewNames[i]}>
                  {previewNames[i]}
                </span>
                <span className="pv-cell-note">
                  {!f.compress_supported && compressEnabled ? "仅重命名" : ""}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {phase === "scanned" && files.length === 0 && (
        <div className="pv-empty">文件夹中没有可处理的图片文件</div>
      )}

      {/* 结果 */}
      {phase === "done" && result && (
        <div className="pv-card">
          <div className={`pv-result-banner ${result.failed.length > 0 ? "pv-result-banner--warn" : ""}`}>
            {result.failed.length === 0
              ? `✓ 处理完成，共 ${result.processed} 个文件 → ${result.output_folder}`
              : `已处理 ${result.processed} 个，${result.failed.length} 个失败`}
            {result.skipped_compress > 0 &&
              `（其中 ${result.skipped_compress} 个 HEIF 等格式跳过压缩）`}
          </div>
          {result.failed.length > 0 && (
            <div className="sv-error" style={{ marginTop: 8 }}>
              失败文件：{result.failed.join("、")}
            </div>
          )}
          <div className="pv-actions" style={{ marginTop: 8 }}>
            <button className="sv-btn sv-btn--ghost" onClick={resetState}>
              重新处理
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
