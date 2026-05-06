pub mod transfer;

use serde::Serialize;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tauri::Emitter;

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

/// 判断扩展名是否为 HEIF 系列格式
#[cfg(windows)]
pub fn is_heif_ext(ext: &str) -> bool {
    matches!(ext, ".hif" | ".heic" | ".heif" | ".avif")
}

/// 通过 Windows WIC API 解码 HEIF/HIF 格式，输出 RGB 图像
/// 需要系统安装「HEIF 图像扩展」（Microsoft Store 免费）
/// 未安装扩展或解码失败时返回 Err，调用方应回退到直接复制
#[cfg(windows)]
pub fn decode_heif_via_wic(path: &std::path::Path) -> Result<image::RgbImage, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::{Interface, PCWSTR},
        Win32::{
            Foundation::GENERIC_ACCESS_RIGHTS,
            Graphics::Imaging::{
                CLSID_WICImagingFactory, GUID_WICPixelFormat24bppBGR,
                IWICBitmapSource, IWICFormatConverter, IWICImagingFactory,
                WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut,
                WICDecodeMetadataCacheOnDemand,
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER,
                COINIT_MULTITHREADED,
            },
        },
    };
    unsafe {
        // 初始化 COM（S_OK = 新初始化，S_FALSE = 已初始化，均可继续）
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        // 创建 WIC 工厂
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("WIC 工厂创建失败: {e}"))?;

        // 将路径转为 UTF-16 宽字符串
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // 创建解码器（若未安装 HEIF 图像扩展则此处返回 Err）
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,                                    // 不限制编解码器厂商
                GENERIC_ACCESS_RIGHTS(0x8000_0000u32),   // GENERIC_READ
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|e| format!("WIC 解码失败（可能未安装 HEIF 图像扩展）: {e}"))?;

        // 获取第一帧
        let frame = decoder
            .GetFrame(0)
            .map_err(|e| format!("获取图片帧失败: {e}"))?;

        // 获取宽高
        let mut width = 0u32;
        let mut height = 0u32;
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|e| format!("获取图片尺寸失败: {e}"))?;

        // 创建格式转换器，目标格式为 24bpp BGR
        let converter: IWICFormatConverter = factory
            .CreateFormatConverter()
            .map_err(|e| format!("创建格式转换器失败: {e}"))?;

        // IWICBitmapFrameDecode 转为 IWICBitmapSource（需要 Interface trait）
        let source: IWICBitmapSource =
            frame.cast().map_err(|e| format!("接口转换失败: {e}"))?;

        converter
            .Initialize(
                &source,
                &GUID_WICPixelFormat24bppBGR,
                WICBitmapDitherTypeNone,
                None,
                0.0f64,
                WICBitmapPaletteTypeMedianCut,
            )
            .map_err(|e| format!("像素格式转换初始化失败: {e}"))?;

        // 读取像素数据到缓冲区（CopyPixels 签名：prc, stride, &mut [u8]）
        let stride = width * 3;
        let buf_size = (stride * height) as usize;
        let mut buffer = vec![0u8; buf_size];
        converter
            .CopyPixels(
                std::ptr::null(),
                stride,
                &mut buffer,
            )
            .map_err(|e| format!("像素数据读取失败: {e}"))?;

        // WIC 返回 BGR，转换为 RGB（image crate 需要）
        for pixel in buffer.chunks_exact_mut(3) {
            pixel.swap(0, 2);
        }

        image::RgbImage::from_raw(width, height, buffer)
            .ok_or_else(|| "图像缓冲区大小不匹配".to_string())
    }
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
pub struct DateConfig {
    year: u16,
    month: u8,
    day: u8,
}

#[derive(Deserialize, Clone)]
pub struct PipelineStepArg {
    step_type: String,              // "rename" | "compress" | "set_date"
    enabled: bool,
    rename: Option<RenameConfig>,
    compress: Option<CompressConfig>,
    date: Option<DateConfig>,
}

#[derive(Deserialize)]
pub struct ExecuteProcessArgs {
    folder: String,
    output_subdir: String,
    in_place: bool,
    steps: Vec<PipelineStepArg>,
}

/// 处理取消标志，通过 Tauri 状态共享
pub struct CancelFlag(Arc<AtomicBool>);

#[derive(Serialize)]
pub struct ExecuteProcessResult {
    processed: u32,
    skipped_compress: u32,
    wic_converted: u32,
    failed: Vec<String>,
    output_folder: String,
    cancelled: bool,
}

/// 取消正在进行的处理任务
#[tauri::command]
fn cancel_process(state: tauri::State<CancelFlag>) {
    state.0.store(true, std::sync::atomic::Ordering::Relaxed);
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
                    // HEIF 系列：WIC 解码后转 JPEG
                    #[cfg(windows)]
                    if is_heif_ext(&out_ext) {
                        out_ext = ".jpg".to_string();
                    }
                }
            }
            _ => {}
        }
    }
    format!("{}{}", stem, out_ext)
}

/// 从图片文件的 EXIF 中提取时间部分（HH:MM:SS），失败时返回 "00:00:00"
fn read_exif_time(path: &std::path::Path) -> String {
    use little_exif::metadata::Metadata;
    use little_exif::exif_tag::ExifTag;
    let Ok(metadata) = Metadata::new_from_path(path) else { return "00:00:00".to_string() };
    for tag in metadata.data() {
        if let ExifTag::DateTimeOriginal(s) = tag {
            if s.len() >= 19 { return s[11..19].to_string(); }
        }
    }
    for tag in metadata.data() {
        if let ExifTag::ModifyDate(s) = tag {
            if s.len() >= 19 { return s[11..19].to_string(); }
        }
    }
    "00:00:00".to_string()
}

/// 将 DateTimeOriginal / DateTime / DateTimeDigitized 的年月日改为 dc 指定值，时分秒保持 src 原始值
fn apply_exif_date(dest: &std::path::Path, src: &std::path::Path, dc: &DateConfig) {
    use little_exif::metadata::Metadata;
    use little_exif::exif_tag::ExifTag;
    let time = read_exif_time(src);
    let date_str = format!("{:04}:{:02}:{:02} {}", dc.year, dc.month, dc.day, time);
    let Ok(mut metadata) = Metadata::new_from_path(dest) else { return };
    metadata.set_tag(ExifTag::DateTimeOriginal(date_str.clone()));
    metadata.set_tag(ExifTag::ModifyDate(date_str.clone()));
    metadata.set_tag(ExifTag::CreateDate(date_str));
    let _ = metadata.write_to_file(dest);
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
async fn execute_process(app: tauri::AppHandle, state: tauri::State<'_, CancelFlag>, args: ExecuteProcessArgs) -> Result<ExecuteProcessResult, String> {
    // 重置取消标志
    state.0.store(false, std::sync::atomic::Ordering::Relaxed);
    let cancel_flag = Arc::clone(&state.0);
    tauri::async_runtime::spawn_blocking(move || {
    use rayon::prelude::*;
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicU32, Ordering};

    let base = std::path::Path::new(&args.folder);
    let out_dir = if args.in_place {
        base.to_path_buf()
    } else {
        let d = base.join(&args.output_subdir);
        std::fs::create_dir_all(&d)
            .map_err(|e| format!("无法创建输出文件夹: {}", e))?;
        d
    };;

    let files = scan_process(args.folder.clone())?;
    let total = files.len() as u32;

    // 提取 compress 配置（所有文件共用）
    let compress_cfg: Option<CompressConfig> = args.steps.iter()
        .find(|s| s.step_type == "compress" && s.enabled)
        .and_then(|s| s.compress.clone());
    // 提取 set_date 配置
    let date_cfg: Option<DateConfig> = args.steps.iter()
        .find(|s| s.step_type == "set_date" && s.enabled)
        .and_then(|s| s.date.clone());
    let in_place = args.in_place;

    let processed     = Arc::new(AtomicU32::new(0));
    let skipped_cnt   = Arc::new(AtomicU32::new(0));
    let wic_cnt       = Arc::new(AtomicU32::new(0));
    let done_cnt      = Arc::new(AtomicU32::new(0));
    let failed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    files.par_iter().enumerate().for_each(|(index, file_info)| {
        // 检查取消标志，已取消则跳过当前文件
        if cancel_flag.load(Ordering::Relaxed) { return; }

        use image::io::Reader as ImageReader;
        use image::codecs::jpeg::JpegEncoder;

        let src_path = base.join(&file_info.name);
        let out_name = compute_output_name(&file_info.name, index, &args.steps);
        let dest_path = out_dir.join(&out_name);

        let mut file_wic_converted = false;

        let result: Result<std::path::PathBuf, String> = (|| {
            if let Some(ref cc) = compress_cfg {
                if !file_info.compress_supported {
                    #[cfg(windows)]
                    {
                        let ext = std::path::Path::new(&file_info.name)
                            .extension()
                            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                            .unwrap_or_default();
                        if is_heif_ext(&ext) {
                            match decode_heif_via_wic(&src_path) {
                                Ok(rgb_img) => {
                                    let stem_end = out_name.rfind('.').unwrap_or(out_name.len());
                                    let jpg_dest = out_dir.join(format!("{}.jpg", &out_name[..stem_end]));
                                    let mut buf = Vec::new();
                                    JpegEncoder::new_with_quality(&mut buf, cc.jpeg_quality)
                                        .encode_image(&rgb_img)
                                        .map_err(|e| e.to_string())?;
                                    std::fs::write(&jpg_dest, &buf).map_err(|e| e.to_string())?;
                                    file_wic_converted = true;
                                    return Ok(jpg_dest);
                                }
                                Err(_) => {
                                    std::fs::copy(&src_path, &dest_path).map_err(|e| e.to_string())?;
                                    return Ok(dest_path.clone());
                                }
                            }
                        }
                    }
                    std::fs::copy(&src_path, &dest_path).map_err(|e| e.to_string())?;
                    return Ok(dest_path.clone());
                }

                let dot = file_info.name.rfind('.');
                let ext = dot
                    .map(|d| file_info.name[d..].to_lowercase())
                    .unwrap_or_default();

                if ext == ".png" && cc.png_mode == "lossless" {
                    let img = ImageReader::open(&src_path)
                        .map_err(|e| e.to_string())?
                        .decode()
                        .map_err(|e| e.to_string())?;
                    img.save(&dest_path).map_err(|e| e.to_string())?;
                } else {
                    let quality = if ext == ".png" { cc.png_jpeg_quality } else { cc.jpeg_quality };
                    let img = ImageReader::open(&src_path)
                        .map_err(|e| e.to_string())?
                        .decode()
                        .map_err(|e| e.to_string())?;
                    let rgb = img.to_rgb8();
                    let mut buf: Vec<u8> = Vec::new();
                    JpegEncoder::new_with_quality(&mut buf, quality)
                        .encode_image(&rgb)
                        .map_err(|e| e.to_string())?;
                    std::fs::write(&dest_path, &buf).map_err(|e| e.to_string())?;
                }
            } else {
                // 无压缩步骤：src==dest(原地无重命名)则跳过 copy；否则复制
                if src_path != dest_path {
                    std::fs::copy(&src_path, &dest_path).map_err(|e| e.to_string())?;
                }
            }
            Ok(dest_path)
        })();

        match result {
            Ok(actual_path) => {
                processed.fetch_add(1, Ordering::Relaxed);
                if file_wic_converted {
                    wic_cnt.fetch_add(1, Ordering::Relaxed);
                } else if compress_cfg.is_some() && !file_info.compress_supported {
                    skipped_cnt.fetch_add(1, Ordering::Relaxed);
                }
                if let Some(ref dc) = date_cfg {
                    apply_exif_date(&actual_path, &src_path, dc);
                }
                // 原地模式：文件被重命名/转换时删除原文件
                if in_place && actual_path != src_path {
                    let _ = std::fs::remove_file(&src_path);
                }
            }
            Err(_) => {
                failed.lock().unwrap().push(file_info.name.clone());
            }
        }

        let current = done_cnt.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = app.emit("process-progress", serde_json::json!({
            "current": current,
            "total": total,
        }));
    });

    let failed = Arc::try_unwrap(failed).unwrap().into_inner().unwrap();

    let cancelled = cancel_flag.load(Ordering::Relaxed);
    Ok(ExecuteProcessResult {
        processed:       processed.load(Ordering::Relaxed),
        skipped_compress: skipped_cnt.load(Ordering::Relaxed),
        wic_converted:   wic_cnt.load(Ordering::Relaxed),
        failed,
        output_folder: out_dir.to_string_lossy().to_string(),
        cancelled,
    })
    }).await.map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(CancelFlag(Arc::new(AtomicBool::new(false))))
        .manage(transfer::TransferState::new())
    .invoke_handler(tauri::generate_handler![
            scan_folder,
            delete_to_trash,
            delete_permanently,
            scan_classify,
            execute_classify,
            scan_process,
            execute_process,
            cancel_process,
            transfer::start_transfer,
            transfer::stop_transfer,
            transfer::get_transfer_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
