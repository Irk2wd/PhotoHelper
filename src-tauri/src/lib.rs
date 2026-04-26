use serde::Serialize;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
    .invoke_handler(tauri::generate_handler![scan_folder, delete_to_trash, delete_permanently])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
