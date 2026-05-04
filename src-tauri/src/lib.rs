use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize)]
pub struct FileEntry {
    name: String,
    is_file: bool,
}

/// 列出指定文件夹的直接子项（不递归），返回文件名和是否为文件
#[tauri::command]
fn scan_folder(path: String) -> Result<Vec<FileEntry>, String> {
    let dir = fs::read_dir(&path).map_err(|e| format!("无法读取文件夹: {}", e))?;
    let mut entries = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        entries.push(FileEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            is_file: metadata.is_file(),
        });
    }
    Ok(entries)
}

/// 将指定路径列表的文件移入系统回收站，返回失败的路径列表
#[tauri::command]
fn delete_to_trash(paths: Vec<String>) -> Vec<String> {
    let mut failed = Vec::new();
    for path_str in &paths {
        if let Err(_) = trash::delete(Path::new(path_str)) {
            failed.push(path_str.clone());
        }
    }
    failed
}

fn delete_path_permanently(path: &Path) -> std::io::Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// 将指定路径列表的文件直接彻底删除，返回失败的路径列表
#[tauri::command]
fn delete_permanently(paths: Vec<String>) -> Vec<String> {
    let mut failed = Vec::new();
    for path_str in &paths {
        if let Err(_) = delete_path_permanently(Path::new(path_str)) {
            failed.push(path_str.clone());
        }
    }
    failed
}

// ===== 照片分类功能 =====

/// 返回扩展名对应的分类名（固定字符串，供内部逻辑使用）
fn get_photo_category(ext: &str) -> &'static str {
    match ext {
        ".arw" | ".cr3" | ".cr2" | ".nef" | ".raf" | ".dng" | ".orf" | ".rw2" | ".raw"
        | ".3fr" | ".mef" | ".mrw" | ".nrw" | ".pef" | ".srw" | ".x3f" => "RAW",
        ".hif" | ".heic" | ".heif" | ".avif" => "HEIF",
        ".jpg" | ".jpeg" => "JPEG",
        ".png" => "PNG",
        ".tif" | ".tiff" => "TIFF",
        ".mp4" | ".mov" | ".avi" | ".mkv" | ".m4v" | ".mts" | ".m2ts" | ".wmv" => "Video",
        _ => "Other",
    }
}

#[derive(Serialize, Clone)]
pub struct CategoryInfo {
    category: String,
    files: Vec<String>,
}

#[derive(Serialize)]
pub struct ExecuteClassifyResult {
    categories: Vec<CategoryInfo>,
    failed: Vec<String>,
}

/// 读取文件夹，按类型分组，不移动文件
#[tauri::command]
fn scan_classify(folder: String) -> Result<Vec<CategoryInfo>, String> {
    let path = Path::new(&folder);
    let dir = fs::read_dir(path).map_err(|e| format!("无法读取文件夹: {}", e))?;

    let mut map: HashMap<&'static str, Vec<String>> = HashMap::new();
    for entry in dir {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.metadata().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = Path::new(&name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        map.entry(get_photo_category(&ext)).or_default().push(name);
    }

    let mut result: Vec<CategoryInfo> = map
        .into_iter()
        .map(|(cat, mut files)| {
            files.sort();
            CategoryInfo { category: cat.to_string(), files }
        })
        .collect();
    result.sort_by(|a, b| a.category.cmp(&b.category));
    Ok(result)
}

/// 在文件夹下创建子文件夹并移动文件
#[tauri::command]
fn execute_classify(folder: String) -> Result<ExecuteClassifyResult, String> {
    let path = Path::new(&folder);
    // 先收集，再移动（避免边读边写）
    let preview = scan_classify(folder.clone())?;

    let mut moved_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut failed: Vec<String> = Vec::new();

    for cat_info in &preview {
        if cat_info.files.is_empty() {
            continue;
        }
        let dest_dir = path.join(&cat_info.category);
        if let Err(e) = fs::create_dir_all(&dest_dir) {
            return Err(format!("创建文件夹 {} 失败: {}", cat_info.category, e));
        }
        for file_name in &cat_info.files {
            let src = path.join(file_name);
            let dest = dest_dir.join(file_name);
            if dest.exists() {
                // 目标已存在：跳过，但仍记录到结果
                moved_map.entry(cat_info.category.clone()).or_default().push(file_name.clone());
                continue;
            }
            match fs::rename(&src, &dest) {
                Ok(_) => {
                    moved_map.entry(cat_info.category.clone()).or_default().push(file_name.clone());
                }
                Err(_) => {
                    failed.push(file_name.clone());
                }
            }
        }
    }

    let mut categories: Vec<CategoryInfo> = moved_map
        .into_iter()
        .map(|(cat, files)| CategoryInfo { category: cat, files })
        .collect();
    categories.sort_by(|a, b| a.category.cmp(&b.category));

    Ok(ExecuteClassifyResult { categories, failed })
}

// ===== 图片批处理功能 =====

/// 判断扩展名是否可被 image crate 压缩
fn compress_supported(ext: &str) -> bool {
    matches!(ext, ".jpg" | ".jpeg" | ".png" | ".webp" | ".bmp" | ".tif" | ".tiff")
}

/// 判断扩展名是否为可处理的图片（含不可压缩的 HEIF）
fn is_processable_image(ext: &str) -> bool {
    matches!(
        ext,
        ".jpg" | ".jpeg" | ".png" | ".webp" | ".bmp" | ".tif" | ".tiff"
            | ".hif" | ".heic" | ".heif" | ".avif"
    )
}

#[derive(Serialize)]
pub struct ProcessFileInfo {
    name: String,
    size_bytes: u64,
    compress_supported: bool,
}

#[derive(Deserialize)]
pub struct RenameConfig {
    /// "prefix_seq" | "keep_suffix" | "custom_seq"
    pattern: String,
    prefix: String,
    suffix: String,
    start_num: u32,
    pad_digits: u8,
}

#[derive(Deserialize)]
pub struct CompressConfig {
    jpeg_quality: u8,
    /// "lossless" | "to_jpeg"
    png_mode: String,
    png_jpeg_quality: u8,
}

#[derive(Deserialize)]
pub struct ExecuteProcessArgs {
    folder: String,
    output_subdir: String,
    rename: Option<RenameConfig>,
    compress: Option<CompressConfig>,
}

#[derive(Serialize)]
pub struct ExecuteProcessResult {
    processed: u32,
    skipped_compress: u32,
    failed: Vec<String>,
    output_folder: String,
}

/// 扫描文件夹，返回可处理的图片文件信息
#[tauri::command]
fn scan_process(folder: String) -> Result<Vec<ProcessFileInfo>, String> {
    let path = Path::new(&folder);
    let dir = fs::read_dir(path).map_err(|e| format!("无法读取文件夹: {}", e))?;

    let mut files: Vec<ProcessFileInfo> = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = Path::new(&name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        if is_processable_image(&ext) {
            files.push(ProcessFileInfo {
                name,
                size_bytes: metadata.len(),
                compress_supported: compress_supported(&ext),
            });
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// 计算输出文件名
fn compute_output_name(
    original: &str,
    index: usize,
    rename: &Option<RenameConfig>,
    compress: &Option<CompressConfig>,
) -> String {
    let orig_path = Path::new(original);
    let stem = orig_path.file_stem().unwrap_or_default().to_string_lossy();
    let ext_lower = orig_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    // 计算输出扩展名（压缩模块可能把 .png 转为 .jpg）
    let out_ext = if let Some(c) = compress {
        if ext_lower == ".png" && c.png_mode == "to_jpeg" {
            ".jpg".to_string()
        } else {
            ext_lower.clone()
        }
    } else {
        ext_lower.clone()
    };

    // 计算输出文件名主干
    let out_stem = if let Some(r) = rename {
        let pad = r.pad_digits as usize;
        let num = r.start_num as usize + index;
        match r.pattern.as_str() {
            "prefix_seq" => format!("{}{:0>pad$}", r.prefix, num, pad = pad),
            "keep_suffix" => format!("{}{}", stem, r.suffix),
            "custom_seq" => format!("{}{:0>pad$}{}", r.prefix, num, r.suffix, pad = pad),
            _ => stem.to_string(),
        }
    } else {
        stem.to_string()
    };

    format!("{}{}", out_stem, out_ext)
}

/// 执行图片批处理（重命名 + 压缩），输出到子文件夹
#[tauri::command]
fn execute_process(args: ExecuteProcessArgs) -> Result<ExecuteProcessResult, String> {
    let folder_path = Path::new(&args.folder);
    let output_subdir = if args.output_subdir.trim().is_empty() {
        "processed"
    } else {
        args.output_subdir.trim()
    };
    let out_dir = folder_path.join(output_subdir);
    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("创建输出文件夹失败: {}", e))?;

    // 扫描文件列表（已排序）
    let scan = scan_process(args.folder.clone())?;

    let mut processed: u32 = 0;
    let mut skipped_compress: u32 = 0;
    let mut failed: Vec<String> = Vec::new();

    for (idx, info) in scan.iter().enumerate() {
        let src = folder_path.join(&info.name);
        let out_name = compute_output_name(&info.name, idx, &args.rename, &args.compress);
        let dest = out_dir.join(&out_name);

        let ext_lower = Path::new(&info.name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();

        let do_compress = args.compress.is_some() && info.compress_supported;

        let ok = if do_compress {
            let compress_cfg = args.compress.as_ref().unwrap();
            match image::open(&src) {
                Err(_) => false,
                Ok(img) => {
                    let is_png_lossless = ext_lower == ".png" && compress_cfg.png_mode != "to_jpeg";
                    if is_png_lossless {
                        img.save(&dest).is_ok()
                    } else {
                        // JPEG 输出（JPEG / WebP / BMP / TIFF 原格式 → JPEG，PNG → JPEG）
                        let quality = if ext_lower == ".png" {
                            compress_cfg.png_jpeg_quality
                        } else {
                            compress_cfg.jpeg_quality
                        };
                        let mut buf: Vec<u8> = Vec::new();
                        let encoder =
                            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
                        if img.write_with_encoder(encoder).is_ok() {
                            fs::write(&dest, &buf).is_ok()
                        } else {
                            false
                        }
                    }
                }
            }
        } else {
            // 不压缩或格式不支持：直接复制
            if !info.compress_supported && args.compress.is_some() {
                skipped_compress += 1;
            }
            fs::copy(&src, &dest).is_ok()
        };

        if ok {
            processed += 1;
        } else {
            failed.push(info.name.clone());
        }
    }

    Ok(ExecuteProcessResult {
        processed,
        skipped_compress,
        failed,
        output_folder: out_dir.to_string_lossy().to_string(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
    .invoke_handler(tauri::generate_handler![
            scan_folder,
            delete_to_trash,
            delete_permanently,
            scan_classify,
            execute_classify,
            scan_process,
            execute_process,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
