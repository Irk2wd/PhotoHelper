/* ===== 类型定义：HEIF/RAW 同步功能 ===== */

/** 所有可选的 HEIF/高效图像 扩展名（含描述） */
export const HEIF_EXT_OPTIONS: { ext: string; label: string }[] = [
  { ext: ".hif",  label: ".hif  (Canon HIF / HEIF)" },
  { ext: ".heic", label: ".heic (Apple HEIC)" },
  { ext: ".heif", label: ".heif (通用 HEIF)" },
  { ext: ".avif", label: ".avif (AVIF)" },
];

/** 所有可选的 RAW 扩展名（含描述） */
export const RAW_EXT_OPTIONS: { ext: string; label: string }[] = [
  { ext: ".arw", label: ".arw (Sony ARW)" },
  { ext: ".cr3", label: ".cr3 (Canon CR3)" },
  { ext: ".cr2", label: ".cr2 (Canon CR2)" },
  { ext: ".nef", label: ".nef (Nikon NEF)" },
  { ext: ".raf", label: ".raf (Fujifilm RAF)" },
  { ext: ".dng", label: ".dng (Adobe DNG)" },
  { ext: ".orf", label: ".orf (Olympus ORF)" },
  { ext: ".rw2", label: ".rw2 (Panasonic RW2)" },
];

/** 默认选中的 HEIF 扩展名 */
export const DEFAULT_HEIF_EXTS = new Set([".hif"]);

/** 默认选中的 RAW 扩展名 */
export const DEFAULT_RAW_EXTS = new Set([".arw"]);

/** 文件配对状态 */
export type PairStatus = "paired" | "heif_only" | "raw_only";

/** 一组同名照片的配对信息 */
export interface PhotoPair {
  /** 文件名主干（不含扩展名） */
  stem: string;
  /** HEIF 文件的完整路径（不存在则为 undefined） */
  heifPath?: string;
  /** RAW 文件的完整路径（不存在则为 undefined） */
  rawPath?: string;
  /** 配对状态 */
  status: PairStatus;
}

/** 文件夹模式 */
export type FolderMode = "split" | "mixed";

/** 同步方向 */
export type SyncMode = "heif_to_raw" | "raw_to_heif" | "bidirectional";

/** 扫描配置 */
export interface ScanConfig {
  folderMode: FolderMode;
  /** mixed 模式使用，或 split 模式下的 HEIF 文件夹 */
  heifFolder: string;
  /** split 模式下的 RAW 文件夹 */
  rawFolder: string;
  /** 用户选中的 HEIF 侧扩展名（小写，含点） */
  heifExts: Set<string>;
  /** 用户选中的 RAW 侧扩展名（小写，含点） */
  rawExts: Set<string>;
}

/** Rust scan_folder 命令返回的条目 */
export interface FileEntry {
  name: string;
  is_file: boolean;
}

/** 扫描统计 */
export interface ScanStats {
  total: number;
  paired: number;
  heifOnly: number;
  rawOnly: number;
}
