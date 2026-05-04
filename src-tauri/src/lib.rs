use serde::Serialize;
use serde::Deserialize;
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

// ===== 图片处理 Pipeline =====

/// 扩展名是否支持用 image crate 压缩
fn compress_supported(ext: &str) -> bool {
    matches!(ext, ".jpg" | ".jpeg" | ".png" | ".webp" | ".bmp" | ".tif" | ".tiff")
}

/// 扩展名是否为可处理的图片（包含不支持压缩的 HEIF）
fn is_processable_image(ext: &str) -> bool {
    matches!(
        ext,
        ".jpg" | ".jpeg" | ".png" | ".webp" | ".bmp" | ".tif" | ".tiff"
            | ".hif" | ".heic" | ".heif" | ".avif"
    )
}

#[derive(Serialize, Clone)]
pub struct ProcessFileInfo {
    name: String,
    size_bytes: u64,
    compress_supported: bool,
}

#[derive(Deserialize, Clone)]
pub struct RenameConfig {
    pattern: String,   // "prefix_seq" | "keep_suffix" | "custom_seq"
    prefix: String,
    suffix: String,
    start_num: u32,
    pad_digits: u8,
}

#[derive(Deserialize, Clone)]
pub struct CompressConfig {
    jpeg_quality: u8,
    png_mode: String,        // "lossless" | "to_jpeg"
    png_jpeg_quality: u8,
}

#[derive(Deserialize, Clone)]
pub struct PipelineStepArg {
    step_type: String,              // "rename" | "compress"
    enabled: bool,
    rename: Option<RenameConfig>,
    compress: Option<CompressConfig>,
}

#[derive(Deserialize)]
pub struct ExecuteProcessArgs {
    folder: String,
    output_subdir: String,
    steps: Vec<PipelineStepArg>,
}

#[derive(Serialize)]
pub struct ExecuteProcessResult {
    processed: u32,
    skipped_compress: u32,
    failed: Vec<String>,
    output_folder: String,
}

/// 按 pipeline steps 依次计算输出文件名（纯函数，前后端逻辑对称）
fn compute_output_name(original: &str, index: usize, steps: &[PipelineStepArg]) -> String {
    let dot = original.rfind('.');
    let mut stem = if let Some(d) = dot { original[..d].to_string() } else { original.to_string() };
    let ext_raw = if let Some(d) = dot { original[d..].to_lowercase() } else { String::new() };
    let mut out_ext = ext_raw.clone();

    for step in steps {
        if !step.enabled {
            continue;
        }
        match step.step_type.as_str() {
            "rename" => {
                if let Some(rc) = &step.rename {
                    let num = format!(
                        "{:0>width$}",
                        rc.start_num as usize + index,
                        width = rc.pad_digits as usize
                    );
                    stem = match rc.pattern.as_str() {
                        "prefix_seq"  => format!("{}{}", rc.prefix, num),
                        "keep_suffix" => format!("{}{}", stem, rc.suffix),
                        "custom_seq"  => format!("{}{}{}", rc.prefix, num, rc.suffix),
                        _             => stem,
                    };
                }
            }
            "compress" => {
                if let Some(cc) = &step.compress {
                    if out_ext == ".png" && cc.png_mode == "to_jpeg" {
                        out_ext = ".jpg".to_string();
                    }
                    // 其他可压缩格式转 .jpg
                    if compress_supported(&out_ext) && out_ext != ".png" {
                        out_ext = ".jpg".to_string();
                    }
                }
            }
            _ => {}
        }
    }
    format!("{}{}", stem, out_ext)
}

/// 扫描文件夹，返回可处理图片信息列表
#[tauri::command]
fn scan_process(folder: String) -> Result<Vec<ProcessFileInfo>, String> {
    let path = std::path::Path::new(&folder);
    let dir = std::fs::read_dir(path).map_err(|e| format!("无法读取文件夹: {}", e))?;
    let mut files: Vec<ProcessFileInfo> = Vec::new();
    for entry in dir {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = std::path::Path::new(&name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        if !is_processable_image(&ext) {
            continue;
        }
        files.push(ProcessFileInfo {
            name,
            size_bytes: meta.len(),
            compress_supported: compress_supported(&ext),
        });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

/// 按 pipeline 执行处理，输出到 output_subdir
#[tauri::command]
fn execute_process(args: ExecuteProcessArgs) -> Result<ExecuteProcessResult, String> {
    use image::io::Reader as ImageReader;
    use image::codecs::jpeg::JpegEncoder;

    let base = std::path::Path::new(&args.folder);
    let out_dir = base.join(&args.output_subdir);
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("无法创建输出文件夹: {}", e))?;

    // 先扫描文件列表（保证与 scan_process 排序一致）
    let files = scan_process(args.folder.clone())?;
    let active_steps: Vec<&PipelineStepArg> = args.steps.iter().filter(|s| s.enabled).collect();

    let mut processed = 0u32;
    let mut skipped_compress = 0u32;
    let mut failed: Vec<String> = Vec::new();

    for (index, file_info) in files.iter().enumerate() {
        let src_path = base.join(&file_info.name);
        let out_name = compute_output_name(&file_info.name, index, &args.steps);
        let dest_path = out_dir.join(&out_name);

        // 找出 compress step（若存在且 enabled）
        let compress_step = active_steps.iter()
            .find(|s| s.step_type == "compress")
            .and_then(|s| s.compress.as_ref());

        let result: Result<(), String> = (|| {
            if let Some(cc) = compress_step {
                if !file_info.compress_supported {
                    // HEIF 等不支持压缩，直接 copy
                    std::fs::copy(&src_path, &dest_path)
                        .map_err(|e| e.to_string())?;
                    return Ok(());  // skipped_compress 在下面统计
                }

                let dot = file_info.name.rfind('.');
                let ext = dot
                    .map(|d| file_info.name[d..].to_lowercase())
                    .unwrap_or_default();

                if ext == ".png" && cc.png_mode == "lossless" {
                    // PNG 无损：重新编码为 PNG
                    let img = ImageReader::open(&src_path)
                        .map_err(|e| e.to_string())?
                        .decode()
                        .map_err(|e| e.to_string())?;
                    img.save(&dest_path).map_err(|e| e.to_string())?;
                } else {
                    // 其余情况全部输出为 JPEG（PNG to_jpeg / JPEG / WEBP / BMP / TIFF）
                    let quality = if ext == ".png" { cc.png_jpeg_quality } else { cc.jpeg_quality };
                    let img = ImageReader::open(&src_path)
                        .map_err(|e| e.to_string())?
                        .decode()
                        .map_err(|e| e.to_string())?;
                    let rgb = img.to_rgb8();
                    let mut buf: Vec<u8> = Vec::new();
                    let mut enc = JpegEncoder::new_with_quality(&mut buf, quality);
                    enc.encode_image(&rgb).map_err(|e| e.to_string())?;
                    std::fs::write(&dest_path, &buf).map_err(|e| e.to_string())?;
                }
            } else {
                // 没有 compress 步骤：直接 copy（rename 已体现在 dest_path 的文件名中）
                std::fs::copy(&src_path, &dest_path).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();

        match result {
            Ok(_) => {
                processed += 1;
                if compress_step.is_some() && !file_info.compress_supported {
                    skipped_compress += 1;
                }
            }
            Err(_) => {
                failed.push(file_info.name.clone());
            }
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
