use serde::Serialize;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
