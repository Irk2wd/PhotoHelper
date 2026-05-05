import { useState, useMemo, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./ProcessView.css";

// ─── 类型定义 ────────────────────────────────────────────

export interface ProcessFileInfo {
  name: string;
  size_bytes: number;
  compress_supported: boolean;
}

export interface RenameConfig {
  pattern: "prefix_seq" | "keep_suffix" | "custom_seq";
  prefix: string;
  suffix: string;
  start_num: number;
  pad_digits: number;
}

export interface CompressConfig {
  jpeg_quality: number;
  png_mode: "lossless" | "to_jpeg";
  png_jpeg_quality: number;
}

export interface DateConfig {
  year: number;
  month: number;
  day: number;
}

export type StepType = "rename" | "compress" | "set_date";

export interface PipelineStep {
  id: string;               // 前端唯一 id（用于 key + 排序）
  step_type: StepType;
  enabled: boolean;
  rename?: RenameConfig;
  compress?: CompressConfig;
  date?: DateConfig;
}

interface ExecuteProcessResult {
  processed: number;
  skipped_compress: number;
  wic_converted: number;
  failed: string[];
  output_folder: string;
  cancelled: boolean;
}

// ─── 默认配置 ────────────────────────────────────────────

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

function todayDate(): DateConfig {
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate() };
}

// ─── 工具函数 ────────────────────────────────────────────

export function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

/**
 * 预估压缩后大小（前端启发式，与 Rust 逻辑无关，仅供参考）
 */
export function estimateCompressedSize(
  originalBytes: number,
  fileName: string,
  compress: CompressConfig
): number {
  const dot = fileName.lastIndexOf(".");
  const ext = dot >= 0 ? fileName.slice(dot).toLowerCase() : "";
  const q = compress.jpeg_quality / 100;

  // HEIF 系列：经 WIC 解码后转为 JPEG，原始文件通常比同质量 JPEG 小约 50%
  const HEIF_EXTS = [".hif", ".heic", ".heif", ".avif"];
  if (HEIF_EXTS.includes(ext)) {
    return Math.round(originalBytes * 2 * Math.pow(q, 1.2));
  }

  if (!isCompressSupported(ext)) return originalBytes;

  if (ext === ".png") {
    if (compress.png_mode === "lossless") return Math.round(originalBytes * 0.95);
    const pq = compress.png_jpeg_quality / 100;
    return Math.round(originalBytes * pq * 0.35);
  }
  // JPEG / WEBP / BMP / TIFF → JPEG
  return Math.round(originalBytes * Math.pow(q, 1.2));
}

function isCompressSupported(ext: string): boolean {
  return [".jpg", ".jpeg", ".png", ".webp", ".bmp", ".tif", ".tiff"].includes(ext);
}

/**
 * 按 pipeline steps 顺序计算输出文件名（与 Rust compute_output_name 逻辑对称）
 */
export function computeOutputName(
  original: string,
  index: number,
  steps: PipelineStep[]
): string {
  const dotIdx = original.lastIndexOf(".");
  let stem = dotIdx >= 0 ? original.slice(0, dotIdx) : original;
  const extRaw = dotIdx >= 0 ? original.slice(dotIdx).toLowerCase() : "";
  let outExt = extRaw;

  for (const step of steps) {
    if (!step.enabled) continue;
    if (step.step_type === "rename" && step.rename) {
      const rc = step.rename;
      const num = String(rc.start_num + index).padStart(rc.pad_digits, "0");
      switch (rc.pattern) {
        case "prefix_seq":  stem = `${rc.prefix}${num}`; break;
        case "keep_suffix": stem = `${stem}${rc.suffix}`; break;
        case "custom_seq":  stem = `${rc.prefix}${num}${rc.suffix}`; break;
      }
    }
    if (step.step_type === "compress" && step.compress) {
      const cc = step.compress;
      const HEIF_EXTS = [".hif", ".heic", ".heif", ".avif"];
      if (outExt === ".png" && cc.png_mode === "to_jpeg") outExt = ".jpg";
      else if (isCompressSupported(outExt) && outExt !== ".png") outExt = ".jpg";
      else if (HEIF_EXTS.includes(outExt)) outExt = ".jpg"; // WIC 转换
    }
  }
  return `${stem}${outExt}`;
}

function uid(): string {
  return Math.random().toString(36).slice(2);
}

// ─── StepCard 组件 ───────────────────────────────────────

interface StepCardProps {
  step: PipelineStep;
  index: number;
  total: number;
  previewSample: string[];      // 前 3 个预览名
  onChange: (updated: PipelineStep) => void;
  onMove: (direction: "up" | "down") => void;
  onRemove: () => void;
}

function StepCard({ step, index, total, previewSample, onChange, onMove, onRemove }: StepCardProps) {
  const isRename = step.step_type === "rename";
  const isDate = step.step_type === "set_date";
  const rc = step.rename ?? { ...DEFAULT_RENAME };
  const cc = step.compress ?? { ...DEFAULT_COMPRESS };
  const dc = step.date ?? todayDate();

  function setRC(patch: Partial<RenameConfig>) {
    onChange({ ...step, rename: { ...rc, ...patch } });
  }
  function setCC(patch: Partial<CompressConfig>) {
    onChange({ ...step, compress: { ...cc, ...patch } });
  }
  function setDC(patch: Partial<DateConfig>) {
    onChange({ ...step, date: { ...dc, ...patch } });
  }

  return (
    <div className={`pv-step-card ${step.enabled ? "pv-step-card--on" : ""}`}>
      {/* 卡片头 */}
      <div className="pv-step-header">
        <span className="pv-step-index">{index + 1}</span>
        <span className="pv-step-title">
          {isRename ? "重命名" : isDate ? "修改日期" : "压缩"}
        </span>
        <div className="pv-step-actions">
          <button
            className="pv-icon-btn"
            title="上移"
            disabled={index === 0}
            onClick={() => onMove("up")}
          >↑</button>
          <button
            className="pv-icon-btn"
            title="下移"
            disabled={index === total - 1}
            onClick={() => onMove("down")}
          >↓</button>
          <button
            className={`pv-toggle-btn ${step.enabled ? "pv-toggle-btn--on" : ""}`}
            onClick={() => onChange({ ...step, enabled: !step.enabled })}
            aria-pressed={step.enabled}
            title={step.enabled ? "禁用" : "启用"}
          >
            <span className="pv-toggle-knob" />
          </button>
          <button className="pv-icon-btn pv-icon-btn--danger" title="删除步骤" onClick={onRemove}>×</button>
        </div>
      </div>

      {/* 配置体（仅 enabled 时展开） */}
      {step.enabled && (
        <div className="pv-step-body">
          {isDate ? (
            <>
              <div className="pv-field-group">
                <label className="pv-field-label">目标日期（只修改年月日，时分秒保留原始 EXIF）</label>
                <div className="pv-date-row">
                  <input
                    className="pv-input pv-date-input"
                    type="number" min={1900} max={2099}
                    value={dc.year}
                    onChange={e => setDC({ year: Math.min(2099, Math.max(1900, parseInt(e.target.value) || dc.year)) })}
                  />
                  <span className="pv-date-sep">年</span>
                  <input
                    className="pv-input pv-date-input pv-date-input--sm"
                    type="number" min={1} max={12}
                    value={dc.month}
                    onChange={e => setDC({ month: Math.min(12, Math.max(1, parseInt(e.target.value) || dc.month)) })}
                  />
                  <span className="pv-date-sep">月</span>
                  <input
                    className="pv-input pv-date-input pv-date-input--sm"
                    type="number" min={1} max={31}
                    value={dc.day}
                    onChange={e => setDC({ day: Math.min(31, Math.max(1, parseInt(e.target.value) || dc.day)) })}
                  />
                  <span className="pv-date-sep">日</span>
                </div>
              </div>
              <div className="pv-compress-note">
                修改 JPEG / TIFF / RAW 等支持 EXIF 的格式；PNG / BMP 等不含 EXIF 的格式会静默跳过
              </div>
            </>
          ) : isRename ? (
            <>
              {/* 命名模式 */}
              <div className="pv-field-group">
                <label className="pv-field-label">命名模式</label>
                <div className="pv-radio-group">
                  {([
                    ["prefix_seq",  "前缀 + 序号"],
                    ["keep_suffix", "保留原名 + 后缀"],
                    ["custom_seq",  "前缀 + 序号 + 后缀"],
                  ] as [RenameConfig["pattern"], string][]).map(([val, label]) => (
                    <label key={val} className="pv-radio-item">
                      <input type="radio" name={`pattern-${step.id}`}
                        checked={rc.pattern === val}
                        onChange={() => setRC({ pattern: val })}
                      />
                      <span>{label}</span>
                    </label>
                  ))}
                </div>
              </div>

              {/* 前缀 / 后缀 / 起始 / 补零 */}
              <div className="pv-inline-fields">
                {rc.pattern !== "keep_suffix" && (
                  <div className="pv-field-group">
                    <label className="pv-field-label">前缀</label>
                    <input className="pv-input pv-input--sm"
                      value={rc.prefix}
                      onChange={e => setRC({ prefix: e.target.value })}
                      placeholder="photo_"
                    />
                  </div>
                )}
                {rc.pattern !== "prefix_seq" && (
                  <div className="pv-field-group">
                    <label className="pv-field-label">后缀</label>
                    <input className="pv-input pv-input--sm"
                      value={rc.suffix}
                      onChange={e => setRC({ suffix: e.target.value })}
                      placeholder="_edit"
                    />
                  </div>
                )}
                {rc.pattern !== "keep_suffix" && (
                  <>
                    <div className="pv-field-group">
                      <label className="pv-field-label">起始编号</label>
                      <input className="pv-input pv-input--sm" type="number" min={0}
                        value={rc.start_num}
                        onChange={e => setRC({ start_num: Math.max(0, parseInt(e.target.value) || 0) })}
                      />
                    </div>
                    <div className="pv-field-group">
                      <label className="pv-field-label">补零位数</label>
                      <input className="pv-input pv-input--sm" type="number" min={1} max={9}
                        value={rc.pad_digits}
                        onChange={e => setRC({ pad_digits: Math.min(9, Math.max(1, parseInt(e.target.value) || 3)) })}
                      />
                    </div>
                  </>
                )}
              </div>

              {/* 实时预览 */}
              {previewSample.length > 0 && (
                <div className="pv-preview-row">
                  <span className="pv-field-label">预览：</span>
                  <span className="pv-preview-names">
                    {previewSample.join("  ·  ")}
                    {previewSample.length >= 3 && "  …"}
                  </span>
                </div>
              )}
            </>
          ) : (
            <>
              {/* JPEG 质量 */}
              <div className="pv-field-group">
                <label className="pv-field-label">
                  JPEG 质量：<strong>{cc.jpeg_quality}</strong>
                </label>
                <input type="range" min={1} max={100} className="pv-slider"
                  value={cc.jpeg_quality}
                  onChange={e => setCC({ jpeg_quality: +e.target.value })}
                />
                <div className="pv-slider-hints">
                  <span>小文件（低质量）</span><span>高质量（大文件）</span>
                </div>
              </div>

              {/* PNG 模式 */}
              <div className="pv-field-group">
                <label className="pv-field-label">PNG 处理方式</label>
                <div className="pv-radio-group">
                  <label className="pv-radio-item">
                    <input type="radio" name={`png-${step.id}`}
                      checked={cc.png_mode === "lossless"}
                      onChange={() => setCC({ png_mode: "lossless" })}
                    />
                    <span>保持 PNG 格式（无损）</span>
                  </label>
                  <label className="pv-radio-item">
                    <input type="radio" name={`png-${step.id}`}
                      checked={cc.png_mode === "to_jpeg"}
                      onChange={() => setCC({ png_mode: "to_jpeg" })}
                    />
                    <span>转换为 JPEG</span>
                  </label>
                </div>
                {cc.png_mode === "to_jpeg" && (
                  <div className="pv-field-group" style={{ marginTop: 8 }}>
                    <label className="pv-field-label">
                      PNG→JPEG 质量：<strong>{cc.png_jpeg_quality}</strong>
                    </label>
                    <input type="range" min={1} max={100} className="pv-slider"
                      value={cc.png_jpeg_quality}
                      onChange={e => setCC({ png_jpeg_quality: +e.target.value })}
                    />
                  </div>
                )}
              </div>

              <div className="pv-compress-note">
                HEIF / HIF / AVIF 格式将通过 Windows WIC API 转换为 JPEG（需安装「HEIF Image Extensions」）；未安装时直接复制原文件
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

// ─── 主视图 ──────────────────────────────────────────────

type Phase = "idle" | "scanning" | "scanned" | "executing" | "done";

const STEP_LABELS: Record<StepType, string> = {
  rename: "重命名",
  compress: "压缩",
  set_date: "修改日期",
};

export default function ProcessView() {
  const [folder, setFolder] = useState("");
  const [outputSubdir, setOutputSubdir] = useState("processed");
  const [phase, setPhase] = useState<Phase>("idle");
  const [files, setFiles] = useState<ProcessFileInfo[]>([]);
  const [result, setResult] = useState<ExecuteProcessResult | null>(null);
  const [error, setError] = useState("");
  const [progress, setProgress] = useState<{ current: number; total: number } | null>(null);
  const [steps, setSteps] = useState<PipelineStep[]>([
    { id: uid(), step_type: "rename",   enabled: true,  rename: { ...DEFAULT_RENAME } },
    { id: uid(), step_type: "compress", enabled: true,  compress: { ...DEFAULT_COMPRESS } },
  ]);

  const isLoading = phase === "scanning" || phase === "executing";
  const hasCompress = steps.some(s => s.step_type === "compress" && s.enabled);
  const presentTypes = new Set(steps.map(s => s.step_type));

  // ── pipeline 编辑 ──
  const updateStep = useCallback((id: string, updated: PipelineStep) => {
    setSteps(prev => prev.map(s => s.id === id ? updated : s));
  }, []);

  const removeStep = useCallback((id: string) => {
    setSteps(prev => prev.filter(s => s.id !== id));
  }, []);

  const moveStep = useCallback((id: string, dir: "up" | "down") => {
    setSteps(prev => {
      const idx = prev.findIndex(s => s.id === id);
      if (idx < 0) return prev;
      const next = [...prev];
      const swap = dir === "up" ? idx - 1 : idx + 1;
      if (swap < 0 || swap >= next.length) return prev;
      [next[idx], next[swap]] = [next[swap], next[idx]];
      return next;
    });
  }, []);

  function addStep(type: StepType) {
    const newStep: PipelineStep =
      type === "rename"
        ? { id: uid(), step_type: "rename",   enabled: true, rename:   { ...DEFAULT_RENAME } }
        : type === "set_date"
          ? { id: uid(), step_type: "set_date", enabled: true, date: todayDate() }
          : { id: uid(), step_type: "compress", enabled: true, compress: { ...DEFAULT_COMPRESS } };
    setSteps(prev => [...prev, newStep]);
  }

  // ── 预览文件名（useMemo，实时更新） ──
  const previewNames = useMemo(
    () => files.map((f, i) => computeOutputName(f.name, i, steps)),
    [files, steps]
  );

  // ── 预估压缩大小 ──
  const estimatedSizes = useMemo(() => {
    const compressStep = steps.find(s => s.step_type === "compress" && s.enabled);
    if (!compressStep?.compress) return null;
    const cc = compressStep.compress;
    return files.map(f => estimateCompressedSize(f.size_bytes, f.name, cc));
  }, [files, steps]);

  // ── 每个 StepCard 的文件名预览样本（取前 3 个文件） ──
  function getStepPreviewSample(stepIdx: number): string[] {
    if (files.length === 0) return [];
    const stepsUpTo = steps.slice(0, stepIdx + 1);
    return files.slice(0, 3).map((f, i) => computeOutputName(f.name, i, stepsUpTo));
  }

  // ── 操作 ──
  function resetState() {
    setPhase("idle");
    setFiles([]);
    setResult(null);
    setError("");
    setProgress(null);
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

  async function handleCancel() {
    await invoke("cancel_process");
  }

  async function handleExecute() {
    setPhase("executing");
    setError("");
    setProgress({ current: 0, total: files.length });

    const unlisten = await listen<{ current: number; total: number }>(
      "process-progress",
      e => setProgress(e.payload)
    );

    try {
      const data = await invoke<ExecuteProcessResult>("execute_process", {
        args: {
          folder,
          output_subdir: outputSubdir || "processed",
          steps: steps.map(s => ({
            step_type: s.step_type,
            enabled: s.enabled,
            rename: s.rename ?? null,
            compress: s.compress ?? null,
            date: s.date ?? null,
          })),
        },
      });
      setResult(data);
      setPhase("done");
    } catch (e) {
      setError(String(e));
      setPhase("scanned");
    } finally {
      unlisten();
      setProgress(null);
    }
  }

  const enabledCount = steps.filter(s => s.enabled).length;

  return (
    <div className="pv-root">
      {/* 标题 */}
      <div className="pv-header">
        <h1 className="pv-title">图片处理</h1>
        <p className="pv-subtitle">
          自由搭建处理流水线：按需添加步骤（重命名、压缩等），拖拽排序，输出到独立子文件夹，不影响原始文件
        </p>
      </div>

      {/* 文件夹选择 */}
      <div className="pv-card">
        <div className="pv-folder-row">
          <input
            className="pv-input"
            value={folder}
            onChange={e => { setFolder(e.target.value); resetState(); }}
            placeholder="选择或粘贴图片所在文件夹..."
          />
          <button className="sv-btn sv-btn--ghost" onClick={pickFolder} disabled={isLoading}>
            浏览…
          </button>
        </div>
        <div className="pv-output-row">
          <span className="pv-output-label">输出子文件夹：</span>
          <input
            className="pv-input pv-input--sm"
            value={outputSubdir}
            onChange={e => setOutputSubdir(e.target.value)}
            placeholder="processed"
          />
        </div>
      </div>

      {/* Pipeline 步骤列表 */}
      <div className="pv-pipeline-section">
        <div className="pv-pipeline-header">
          <span className="pv-section-label">处理流水线</span>
          <div className="pv-add-step-row">
            {(["rename", "compress", "set_date"] as StepType[]).map(type => (
              <button
                key={type}
                className="sv-btn sv-btn--ghost pv-add-btn"
                disabled={presentTypes.has(type)}
                title={presentTypes.has(type) ? "每种步骤只能添加一次" : undefined}
                onClick={() => addStep(type)}
              >
                + {STEP_LABELS[type]}
              </button>
            ))}
          </div>
        </div>

        {steps.length === 0 ? (
          <div className="pv-empty-pipeline">尚未添加任何步骤，点击上方按钮添加</div>
        ) : (
          <div className="pv-step-list">
            {steps.map((step, idx) => (
              <StepCard
                key={step.id}
                step={step}
                index={idx}
                total={steps.length}
                previewSample={getStepPreviewSample(idx)}
                onChange={updated => updateStep(step.id, updated)}
                onMove={dir => moveStep(step.id, dir)}
                onRemove={() => removeStep(step.id)}
              />
            ))}
          </div>
        )}
      </div>

      {/* 操作按钮 */}
      <div className="pv-card">
        {error && <div className="sv-error">{error}</div>}
        <div className="pv-actions">
          <button
            className="sv-btn sv-btn--ghost"
            disabled={!folder || isLoading}
            onClick={handleScan}
          >
            {phase === "scanning" ? "扫描中…" : "扫描预览"}
          </button>
          {(phase === "scanned" || phase === "executing") && (
            <button
              className="sv-btn sv-btn--primary"
              disabled={files.length === 0 || isLoading || enabledCount === 0}
              onClick={handleExecute}
            >
              {phase === "executing" ? "处理中…" : `执行（${files.length} 个文件）`}
            </button>
          )}
        </div>
        {phase === "scanned" && enabledCount === 0 && (
          <div className="pv-warn">请至少启用一个处理步骤</div>
        )}
        {phase === "executing" && progress && (
          <div className="pv-progress">
            <div className="pv-progress-bar">
              <div
                className="pv-progress-fill"
                style={{ width: `${progress.total > 0 ? Math.round(progress.current / progress.total * 100) : 0}%` }}
              />
            </div>
            <span className="pv-progress-label">
              {progress.current} / {progress.total}
            </span>            <button
              className="sv-btn sv-btn--danger pv-stop-btn"
              onClick={handleCancel}
              title="停止处理"
            >
              停止
            </button>          </div>
        )}
      </div>

      {/* 预览表格 */}
      {(phase === "scanned" || phase === "executing") && files.length > 0 && (
        <div className="pv-card">
          <div className="pv-section-label">
            {files.length} 个文件 · 输出 → {outputSubdir || "processed"}/
          </div>
          <div className="pv-table">
            <div className="pv-table-head" style={{ gridTemplateColumns: hasCompress ? "2fr 70px 90px 2fr 80px" : "2fr 80px 2fr 80px" }}>
              <span>原文件名</span>
              <span>原大小</span>
              {hasCompress && <span title="前端估算值，仅供参考">预估大小 ≈</span>}
              <span>输出文件名</span>
              <span>备注</span>
            </div>
            {files.map((f, i) => {
              const estSize = estimatedSizes ? estimatedSizes[i] : null;
              const fileExt = (f.name.slice(f.name.lastIndexOf(".")) || "").toLowerCase();
              const isHeif = [".hif", ".heic", ".heif", ".avif"].includes(fileExt);
              // HEIF 通过 WIC 可转换，不算"不支持"
              const notSupported = !f.compress_supported && hasCompress && !isHeif;
              return (
                <div key={f.name} className="pv-table-row" style={{ gridTemplateColumns: hasCompress ? "2fr 70px 90px 2fr 80px" : "2fr 80px 2fr 80px" }}>
                  <span className="pv-cell-mono pv-cell-ellipsis" title={f.name}>{f.name}</span>
                  <span className="pv-cell-size">{fmtSize(f.size_bytes)}</span>
                  {hasCompress && (
                    <span className={`pv-cell-size ${notSupported ? "pv-cell-dim" : ""}`}>
                      {notSupported ? "—" : `~${fmtSize(estSize ?? f.size_bytes)}`}
                    </span>
                  )}
                  <span className="pv-cell-mono pv-cell-ellipsis" title={previewNames[i]}>
                    {previewNames[i]}
                  </span>
                  <span className="pv-cell-note">
                    {notSupported ? "WIC→JPEG" : ""}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {phase === "scanned" && files.length === 0 && (
        <div className="pv-empty">文件夹中没有可处理的图片文件</div>
      )}

      {/* 结果 Banner */}
      {phase === "done" && result && (
        <div className="pv-card">
          <div className={`pv-result-banner ${result.cancelled ? "pv-result-banner--warn" : result.failed.length > 0 ? "pv-result-banner--warn" : ""}`}>
            {result.cancelled
              ? `已停止，已处理 ${result.processed} 个文件（剩余文件已跳过）`
              : result.failed.length === 0
                ? `✓ 全部完成，共 ${result.processed} 个文件 → ${result.output_folder}`
                : `已处理 ${result.processed} 个，${result.failed.length} 个失败`}
            {result.wic_converted > 0 &&
              `（${result.wic_converted} 个 HEIF 已通过 WIC 转换为 JPEG）`}
            {result.skipped_compress > 0 &&
              `（${result.skipped_compress} 个格式未安装 HEIF 扩展，已跳过压缩）`}
          </div>
          {result.failed.length > 0 && (
            <div className="sv-error" style={{ marginTop: 8 }}>
              失败：{result.failed.join("、")}
            </div>
          )}
          <div className="pv-actions" style={{ marginTop: 10 }}>
            <button className="sv-btn sv-btn--ghost" onClick={resetState}>重新处理</button>
          </div>
        </div>
      )}
    </div>
  );
}
