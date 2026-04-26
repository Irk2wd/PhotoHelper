import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  PhotoPair,
  ScanConfig,
  SyncMode,
  FileEntry,
  ScanStats,
} from "./types";

// ── 工具函数 ──────────────────────────────────────────

/** 获取文件扩展名（小写，含点） */
function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot).toLowerCase();
}

/** 获取文件名主干（去掉扩展名） */
function stemOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot === -1 ? name : name.slice(0, dot);
}

/** 拼接路径（兼容 Windows 反斜杠） */
function joinPath(dir: string, name: string): string {
  const sep = dir.includes("\\") ? "\\" : "/";
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

/** 扫描单个文件夹，返回文件条目 */
async function readFolder(path: string): Promise<FileEntry[]> {
  return invoke<FileEntry[]>("scan_folder", { path });
}

// ── 核心扫描逻辑 ──────────────────────────────────────

/**
 * 扫描并配对：分离模式
 */
async function scanSplit(
  heifFolder: string,
  rawFolder: string,
  heifExts: Set<string>,
  rawExts: Set<string>,
): Promise<PhotoPair[]> {
  const [heifEntries, rawEntries] = await Promise.all([
    readFolder(heifFolder),
    readFolder(rawFolder),
  ]);

  const heifMap = new Map<string, string>(); // stem → fullPath
  for (const e of heifEntries) {
    if (!e.is_file) continue;
    const ext = extOf(e.name);
    if (heifExts.has(ext)) {
      heifMap.set(stemOf(e.name), joinPath(heifFolder, e.name));
    }
  }

  const rawMap = new Map<string, string>(); // stem → fullPath
  for (const e of rawEntries) {
    if (!e.is_file) continue;
    const ext = extOf(e.name);
    if (rawExts.has(ext)) {
      rawMap.set(stemOf(e.name), joinPath(rawFolder, e.name));
    }
  }

  const stems = new Set([...heifMap.keys(), ...rawMap.keys()]);
  const pairs: PhotoPair[] = [];

  for (const stem of stems) {
    const heifPath = heifMap.get(stem);
    const rawPath = rawMap.get(stem);
    const status = heifPath && rawPath ? "paired" : heifPath ? "heif_only" : "raw_only";
    pairs.push({ stem, heifPath, rawPath, status });
  }

  return pairs.sort((a, b) => a.stem.localeCompare(b.stem));
}

/**
 * 扫描并配对：混合模式
 * HEIF 和 RAW 在同一个文件夹里
 */
async function scanMixed(
  folder: string,
  heifExts: Set<string>,
  rawExts: Set<string>,
): Promise<PhotoPair[]> {
  const entries = await readFolder(folder);

  const heifMap = new Map<string, string>();
  const rawMap = new Map<string, string>();

  for (const e of entries) {
    if (!e.is_file) continue;
    const ext = extOf(e.name);
    const stem = stemOf(e.name);
    const full = joinPath(folder, e.name);
    if (heifExts.has(ext)) heifMap.set(stem, full);
    else if (rawExts.has(ext)) rawMap.set(stem, full);
  }

  const stems = new Set([...heifMap.keys(), ...rawMap.keys()]);
  const pairs: PhotoPair[] = [];

  for (const stem of stems) {
    const heifPath = heifMap.get(stem);
    const rawPath = rawMap.get(stem);
    const status = heifPath && rawPath ? "paired" : heifPath ? "heif_only" : "raw_only";
    pairs.push({ stem, heifPath, rawPath, status });
  }

  return pairs.sort((a, b) => a.stem.localeCompare(b.stem));
}

// ── 同步方向 → 待删列表 ───────────────────────────────

/**
 * 计算将被删除的文件路径列表（dry run，不实际删除）
 * heif_to_raw: 以 HEIF 为准 → 删除孤立 RAW（raw_only）
 * raw_to_heif: 以 RAW 为准  → 删除孤立 HEIF（heif_only）
 * bidirectional: 删除所有孤立文件
 */
export function computeToDelete(pairs: PhotoPair[], mode: SyncMode): string[] {
  const toDelete: string[] = [];
  for (const p of pairs) {
    if (mode === "heif_to_raw" && p.status === "raw_only" && p.rawPath) {
      toDelete.push(p.rawPath);
    } else if (mode === "raw_to_heif" && p.status === "heif_only" && p.heifPath) {
      toDelete.push(p.heifPath);
    } else if (mode === "bidirectional") {
      if (p.status === "raw_only" && p.rawPath) toDelete.push(p.rawPath);
      if (p.status === "heif_only" && p.heifPath) toDelete.push(p.heifPath);
    }
  }
  return toDelete;
}

/** 计算扫描统计 */
export function computeStats(pairs: PhotoPair[]): ScanStats {
  return {
    total: pairs.length,
    paired: pairs.filter((p) => p.status === "paired").length,
    heifOnly: pairs.filter((p) => p.status === "heif_only").length,
    rawOnly: pairs.filter((p) => p.status === "raw_only").length,
  };
}

// ── React Hook ────────────────────────────────────────

export interface SyncEngineState {
  pairs: PhotoPair[];
  isScanning: boolean;
  isExecuting: boolean;
  scanError: string | null;
  lastExecuteResult: ExecuteResult | null;
}

export interface ExecuteResult {
  deleted: number;
  trashed: number;
  permanentlyDeleted: number;
  failed: string[];
}

export interface ExecuteOptions {
  confirmPermanentDelete?: (failedPaths: string[]) => Promise<boolean>;
}

export interface SyncEngineActions {
  scan: (config: ScanConfig) => Promise<void>;
  execute: (toDelete: string[], options?: ExecuteOptions) => Promise<ExecuteResult>;
  reset: () => void;
}

export function useSyncEngine(): SyncEngineState & SyncEngineActions {
  const [pairs, setPairs] = useState<PhotoPair[]>([]);
  const [isScanning, setIsScanning] = useState(false);
  const [isExecuting, setIsExecuting] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const [lastExecuteResult, setLastExecuteResult] = useState<ExecuteResult | null>(null);

  const scan = useCallback(async (config: ScanConfig) => {
    setIsScanning(true);
    setScanError(null);
    setLastExecuteResult(null);
    try {
      let result: PhotoPair[];
      if (config.heifExts.size === 0 || config.rawExts.size === 0) {
        throw new Error("请至少各选一个 HEIF 扩展名和 RAW 扩展名");
      }
      if (config.folderMode === "split") {
        if (!config.heifFolder || !config.rawFolder) {
          throw new Error("请先选择 HEIF 和 RAW 两个文件夹");
        }
        result = await scanSplit(config.heifFolder, config.rawFolder, config.heifExts, config.rawExts);
      } else {
        if (!config.heifFolder) {
          throw new Error("请先选择文件夹");
        }
        result = await scanMixed(config.heifFolder, config.heifExts, config.rawExts);
      }
      setPairs(result);
    } catch (e: any) {
      setScanError(e?.message ?? String(e));
      setPairs([]);
    } finally {
      setIsScanning(false);
    }
  }, []);

  const execute = useCallback(async (toDelete: string[], options?: ExecuteOptions) => {
    setIsExecuting(true);
    try {
      const failedToTrash = await invoke<string[]>("delete_to_trash", { paths: toDelete });
      const trashed = toDelete.length - failedToTrash.length;

      let permanentlyDeleted = 0;
      let failed = failedToTrash;
      if (failedToTrash.length > 0 && options?.confirmPermanentDelete) {
        const shouldDeletePermanently = await options.confirmPermanentDelete(failedToTrash);
        if (shouldDeletePermanently) {
          const permanentDeleteFailed = await invoke<string[]>("delete_permanently", {
            paths: failedToTrash,
          });
          permanentlyDeleted = failedToTrash.length - permanentDeleteFailed.length;
          failed = permanentDeleteFailed;
        }
      }

      const deleted = trashed + permanentlyDeleted;
      const result = { deleted, trashed, permanentlyDeleted, failed };
      setLastExecuteResult(result);

      const deletedSet = new Set(toDelete.filter((path) => !failed.includes(path)));
      // 重新过滤掉已删除的配对
      setPairs((prev) =>
        prev
          .map((p) => {
            const newP = { ...p };
            if (newP.heifPath && deletedSet.has(newP.heifPath)) newP.heifPath = undefined;
            if (newP.rawPath && deletedSet.has(newP.rawPath)) newP.rawPath = undefined;
            newP.status =
              newP.heifPath && newP.rawPath
                ? "paired"
                : newP.heifPath
                ? "heif_only"
                : newP.rawPath
                ? "raw_only"
                : "paired"; // 不会出现两者都空的情况
            return newP;
          })
          .filter((p) => p.heifPath || p.rawPath)
      );
      return result;
    } finally {
      setIsExecuting(false);
    }
  }, []);

  const reset = useCallback(() => {
    setPairs([]);
    setScanError(null);
    setLastExecuteResult(null);
  }, []);

  return {
    pairs,
    isScanning,
    isExecuting,
    scanError,
    lastExecuteResult,
    scan,
    execute,
    reset,
  };
}
