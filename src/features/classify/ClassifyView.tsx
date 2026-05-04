import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import "./ClassifyView.css";

interface CategoryInfo {
  category: string;
  files: string[];
}

interface ExecuteResult {
  categories: CategoryInfo[];
  failed: string[];
}

/** 每个分类的显示颜色标识 */
const CAT_COLOR: Record<string, string> = {
  RAW: "raw",
  HEIF: "heif",
  JPEG: "jpeg",
  PNG: "png",
  TIFF: "tiff",
  Video: "video",
  Other: "other",
};

/** 从文件列表提取唯一扩展名（最多 5 个） */
function sampleExts(files: string[]): string {
  const exts = [...new Set(files.map((f) => f.slice(f.lastIndexOf("."))))];
  return exts.slice(0, 5).join("  ");
}

type Phase = "idle" | "scanning" | "scanned" | "executing" | "done";

export default function ClassifyView() {
  const [folder, setFolder] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [preview, setPreview] = useState<CategoryInfo[]>([]);
  const [result, setResult] = useState<ExecuteResult | null>(null);
  const [error, setError] = useState("");

  const isLoading = phase === "scanning" || phase === "executing";

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected && typeof selected === "string") {
      setFolder(selected);
      resetState();
    }
  }

  function resetState() {
    setPhase("idle");
    setPreview([]);
    setResult(null);
    setError("");
  }

  async function handleScan() {
    if (!folder) return;
    setPhase("scanning");
    setError("");
    try {
      const data = await invoke<CategoryInfo[]>("scan_classify", { folder });
      setPreview(data.filter((c) => c.files.length > 0));
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
      const data = await invoke<ExecuteResult>("execute_classify", { folder });
      setResult(data);
      setPhase("done");
    } catch (e) {
      setError(String(e));
      setPhase("scanned");
    }
  }

  const totalFiles = preview.reduce((s, c) => s + c.files.length, 0);
  const movedCount = result?.categories.reduce((s, c) => s + c.files.length, 0) ?? 0;

  return (
    <div className="cv-root">
      {/* 页面标题 */}
      <div className="cv-header">
        <h1 className="cv-title">照片分类</h1>
        <p className="cv-subtitle">
          扫描文件夹，按文件类型自动创建子文件夹（RAW / HEIF / JPEG / PNG / TIFF / Video / Other）并归类整理
        </p>
      </div>

      {/* 配置卡片 */}
      <div className="cv-card">
        <div className="cv-folder-row">
          <input
            className="cv-input"
            value={folder}
            onChange={(e) => {
              setFolder(e.target.value);
              resetState();
            }}
            placeholder="选择或粘贴要整理的文件夹路径..."
          />
          <button className="sv-btn sv-btn--ghost" onClick={pickFolder} disabled={isLoading}>
            浏览...
          </button>
        </div>

        <div className="cv-actions">
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
              disabled={preview.length === 0 || isLoading}
              onClick={handleExecute}
            >
              {phase === "executing"
                ? "整理中..."
                : `执行分类（${totalFiles} 个文件）`}
            </button>
          )}
        </div>

        {error && <div className="sv-error">{error}</div>}
      </div>

      {/* 预览表格 */}
      {(phase === "scanned" || phase === "executing") && preview.length > 0 && (
        <div className="cv-card">
          <div className="cv-section-label">预览 — 将创建以下子文件夹</div>
          <div className="cv-table">
            {preview.map((cat) => (
              <div key={cat.category} className="cv-table-row">
                <span className={`cv-cat-badge cv-cat-badge--${CAT_COLOR[cat.category] ?? "other"}`}>
                  {cat.category}/
                </span>
                <span className="cv-row-count">{cat.files.length} 个文件</span>
                <span className="cv-row-exts">{sampleExts(cat.files)}</span>
              </div>
            ))}
            <div className="cv-table-footer">
              共 {totalFiles} 个文件，{preview.length} 个类别
            </div>
          </div>
        </div>
      )}

      {phase === "scanned" && preview.length === 0 && (
        <div className="cv-empty">文件夹中没有需要分类的文件</div>
      )}

      {/* 执行结果 */}
      {phase === "done" && result && (
        <div className="cv-card">
          <div className={`cv-result-banner${result.failed.length > 0 ? " cv-result-banner--warn" : ""}`}>
            {result.failed.length === 0
              ? `✓ 分类完成，已整理 ${movedCount} 个文件到 ${result.categories.length} 个子文件夹`
              : `已整理 ${movedCount} 个文件，${result.failed.length} 个文件移动失败`}
          </div>
          {result.categories.length > 0 && (
            <div className="cv-table">
              {result.categories.map((cat) => (
                <div key={cat.category} className="cv-table-row">
                  <span className={`cv-cat-badge cv-cat-badge--${CAT_COLOR[cat.category] ?? "other"}`}>
                    {cat.category}/
                  </span>
                  <span className="cv-row-count">{cat.files.length} 个文件</span>
                </div>
              ))}
            </div>
          )}
          {result.failed.length > 0 && (
            <div className="sv-error" style={{ marginTop: 8 }}>
              以下文件移动失败：{result.failed.join("、")}
            </div>
          )}
          <div className="cv-actions" style={{ marginTop: 8 }}>
            <button className="sv-btn sv-btn--ghost" onClick={resetState}>
              重新整理
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
