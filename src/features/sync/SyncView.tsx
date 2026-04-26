import { useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useSyncEngine, computeToDelete, computeStats } from "./useSyncEngine";
import {
  FolderMode,
  ScanConfig,
  SyncMode,
  HEIF_EXT_OPTIONS,
  RAW_EXT_OPTIONS,
  DEFAULT_HEIF_EXTS,
  DEFAULT_RAW_EXTS,
} from "./types";
import FilePairTable from "./FilePairTable";
import ConfirmDialog from "../../components/ConfirmDialog";
import "./SyncView.css";

const SYNC_MODES: { id: SyncMode; label: string; desc: string }[] = [
  {
    id: "heif_to_raw",
    label: "以 HEIF 为准",
    desc: "删除没有对应 HEIF 的孤立 RAW",
  },
  {
    id: "raw_to_heif",
    label: "以 RAW 为准",
    desc: "删除没有对应 RAW 的孤立 HEIF",
  },
  {
    id: "bidirectional",
    label: "双向同步",
    desc: "删除所有孤立文件，只保留成对",
  },
];

export default function SyncView() {
  const engine = useSyncEngine();

  const [folderMode, setFolderMode] = useState<FolderMode>("split");
  const [heifFolder, setHeifFolder] = useState("");
  const [rawFolder, setRawFolder] = useState("");
  const [syncMode, setSyncMode] = useState<SyncMode>("heif_to_raw");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [fallbackConfirmOpen, setFallbackConfirmOpen] = useState(false);
  const [toDelete, setToDelete] = useState<string[]>([]);
  const [fallbackToDelete, setFallbackToDelete] = useState<string[]>([]);
  const [heifExts, setHeifExts] = useState<Set<string>>(new Set(DEFAULT_HEIF_EXTS));
  const [rawExts, setRawExts] = useState<Set<string>>(new Set(DEFAULT_RAW_EXTS));
  const fallbackConfirmResolverRef = useRef<((confirmed: boolean) => void) | null>(null);

  function toggleExt(set: Set<string>, setFn: (s: Set<string>) => void, ext: string) {
    const next = new Set(set);
    if (next.has(ext)) next.delete(ext); else next.add(ext);
    setFn(next);
  }

  const stats = computeStats(engine.pairs);
  const hasResults = engine.pairs.length > 0;

  async function pickFolder(setter: (p: string) => void) {
    const selected = await open({ directory: true, multiple: false });
    if (selected && typeof selected === "string") setter(selected);
  }

  async function handleScan() {
    const config: ScanConfig = { folderMode, heifFolder, rawFolder, heifExts, rawExts };
    await engine.scan(config);
  }

  function handlePreview() {
    const list = computeToDelete(engine.pairs, syncMode);
    setToDelete(list);
    setConfirmOpen(true);
  }

  function requestPermanentDeleteConfirmation(paths: string[]) {
    setFallbackToDelete(paths);
    setFallbackConfirmOpen(true);
    return new Promise<boolean>((resolve) => {
      fallbackConfirmResolverRef.current = resolve;
    });
  }

  function resolvePermanentDeleteConfirmation(confirmed: boolean) {
    setFallbackConfirmOpen(false);
    setFallbackToDelete([]);
    fallbackConfirmResolverRef.current?.(confirmed);
    fallbackConfirmResolverRef.current = null;
  }

  async function handleConfirm() {
    setConfirmOpen(false);
    await engine.execute(toDelete, {
      confirmPermanentDelete: requestPermanentDeleteConfirmation,
    });
  }

  function renderExecuteResult() {
    const result = engine.lastExecuteResult;
    if (!result) return null;

    if (result.failed.length === 0) {
      if (result.permanentlyDeleted > 0) {
        if (result.trashed > 0) {
          return `✓ 已处理 ${result.deleted} 个文件，其中 ${result.trashed} 个移入回收站，${result.permanentlyDeleted} 个已彻底删除`;
        }
        return `✓ 已彻底删除 ${result.permanentlyDeleted} 个文件`;
      }
      return `✓ 已将 ${result.deleted} 个文件移入回收站`;
    }

    if (result.permanentlyDeleted > 0) {
      return `已处理 ${result.deleted} 个文件，其中 ${result.trashed} 个移入回收站，${result.permanentlyDeleted} 个已彻底删除，仍有 ${result.failed.length} 个失败`;
    }

    return `已移入回收站 ${result.trashed} 个，${result.failed.length} 个失败`;
  }

  return (
    <div className="sv-root">
      {/* 页面标题 */}
      <div className="sv-header">
        <h1 className="sv-title">HEIF / RAW 同步</h1>
        <p className="sv-subtitle">
          扫描文件夹，自动配对同名照片，按选定方向删除孤立文件（移入回收站）
        </p>
      </div>

      {/* 配置卡片 */}
      <div className="sv-card">
        {/* 文件夹模式 */}
        <div className="sv-field-group">
          <label className="sv-label">文件夹模式</label>
          <div className="sv-toggle-group">
            {(["split", "mixed"] as FolderMode[]).map((m) => (
              <button
                key={m}
                className={`sv-toggle ${folderMode === m ? "sv-toggle--active" : ""}`}
                onClick={() => {
                  setFolderMode(m);
                  engine.reset();
                }}
              >
                {m === "split" ? "分离模式（j/ + r/）" : "混合模式（单文件夹）"}
              </button>
            ))}
          </div>
          <p className="sv-hint">
            {folderMode === "split"
              ? "分别选择存放 HEIF 和 RAW 的两个文件夹"
              : "选择 HEIF 和 RAW 混放在一起的文件夹"}
          </p>
        </div>

        {/* 扩展名选择 */}
        <div className="sv-exts-row">
          <div className="sv-exts-group">
            <label className="sv-label">HEIF 侧扩展名</label>
            <div className="sv-checkboxes">
              {HEIF_EXT_OPTIONS.map(({ ext, label }) => (
                <label key={ext} className="sv-checkbox-item">
                  <input
                    type="checkbox"
                    checked={heifExts.has(ext)}
                    onChange={() => toggleExt(heifExts, setHeifExts, ext)}
                  />
                  <span className="sv-checkbox-label">{label}</span>
                </label>
              ))}
            </div>
          </div>
          <div className="sv-exts-group">
            <label className="sv-label">RAW 侧扩展名</label>
            <div className="sv-checkboxes">
              {RAW_EXT_OPTIONS.map(({ ext, label }) => (
                <label key={ext} className="sv-checkbox-item">
                  <input
                    type="checkbox"
                    checked={rawExts.has(ext)}
                    onChange={() => toggleExt(rawExts, setRawExts, ext)}
                  />
                  <span className="sv-checkbox-label">{label}</span>
                </label>
              ))}
            </div>
          </div>
        </div>

        {/* 文件夹路径 */}
        {folderMode === "split" ? (
          <>
            <FolderInput
              label="HEIF 文件夹（j/）"
              value={heifFolder}
              onChange={setHeifFolder}
              onPick={() => pickFolder(setHeifFolder)}
            />
            <FolderInput
              label="RAW 文件夹（r/）"
              value={rawFolder}
              onChange={setRawFolder}
              onPick={() => pickFolder(setRawFolder)}
            />
          </>
        ) : (
          <FolderInput
            label="混合文件夹"
            value={heifFolder}
            onChange={setHeifFolder}
            onPick={() => pickFolder(setHeifFolder)}
          />
        )}

        {/* 同步方向 */}
        <div className="sv-field-group">
          <label className="sv-label">同步方向</label>
          <div className="sv-sync-modes">
            {SYNC_MODES.map((m) => (
              <button
                key={m.id}
                className={`sv-mode-card ${syncMode === m.id ? "sv-mode-card--active" : ""}`}
                onClick={() => setSyncMode(m.id)}
              >
                <span className="sv-mode-label">{m.label}</span>
                <span className="sv-mode-desc">{m.desc}</span>
              </button>
            ))}
          </div>
        </div>

        {/* 扫描 + 执行按钮行 */}
        <div className="sv-actions-top">
          <button
            className="sv-btn sv-btn--primary"
            onClick={handleScan}
            disabled={engine.isScanning}
          >
            {engine.isScanning ? (
              <><span className="sv-spinner" /> 扫描中…</>
            ) : (
              "扫描文件夹"
            )}
          </button>
          {hasResults && (
            <>
              <button
                className="sv-btn sv-btn--ghost"
                onClick={handlePreview}
                disabled={engine.isExecuting}
              >
                预览将删除的文件
              </button>
              <button
                className="sv-btn sv-btn--danger"
                onClick={handlePreview}
                disabled={engine.isExecuting}
              >
                {engine.isExecuting ? (
                  <><span className="sv-spinner" /> 执行中…</>
                ) : (
                  "执行同步"
                )}
              </button>
            </>
          )}
        </div>

        {/* 执行结果 */}
        {engine.lastExecuteResult && (
          <div className={`sv-result ${engine.lastExecuteResult.failed.length > 0 ? "sv-result--warn" : "sv-result--ok"}`}>
            {renderExecuteResult()}
          </div>
        )}

        {/* 错误提示 */}
        {engine.scanError && (
          <div className="sv-error">{engine.scanError}</div>
        )}
      </div>

      {/* 扫描结果 */}
      {hasResults && (
        <>
          {/* 统计栏 */}
          <div className="sv-stats">
            <StatChip label="共" value={stats.total} color="default" />
            <StatChip label="成对" value={stats.paired} color="success" />
            <StatChip label="仅 HEIF" value={stats.heifOnly} color="warning" />
            <StatChip label="仅 RAW" value={stats.rawOnly} color="danger" />
          </div>

          {/* 文件列表 */}
          <FilePairTable pairs={engine.pairs} />
        </>
      )}

      {/* 确认弹窗 */}
      <ConfirmDialog
        open={confirmOpen}
        toDelete={toDelete}
        onCancel={() => setConfirmOpen(false)}
        onConfirm={handleConfirm}
      />
      <ConfirmDialog
        open={fallbackConfirmOpen}
        toDelete={fallbackToDelete}
        mode="permanent"
        onCancel={() => resolvePermanentDeleteConfirmation(false)}
        onConfirm={() => resolvePermanentDeleteConfirmation(true)}
      />
    </div>
  );
}

// ── 子组件 ──────────────────────────────────────────

function FolderInput({
  label,
  value,
  onChange,
  onPick,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  onPick: () => void;
}) {
  return (
    <div className="sv-field-group">
      <label className="sv-label">{label}</label>
      <div className="sv-folder-row">
        <input
          className="sv-input"
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder="输入路径或点击选择…"
          spellCheck={false}
        />
        <button className="sv-btn sv-btn--ghost sv-btn--sm" onClick={onPick}>
          选择
        </button>
      </div>
    </div>
  );
}

function StatChip({
  label,
  value,
  color,
}: {
  label: string;
  value: number;
  color: "default" | "success" | "warning" | "danger";
}) {
  return (
    <div className={`sv-stat sv-stat--${color}`}>
      <span className="sv-stat-value">{value}</span>
      <span className="sv-stat-label">{label}</span>
    </div>
  );
}
