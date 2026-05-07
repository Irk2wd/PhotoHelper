use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, Response as HttpResponse, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use hyper::{body::Incoming, server::conn::http1};
use hyper_util::rt::TokioIo;
use image::ImageFormat;
use little_exif::exif_tag::ExifTag;
use little_exif::metadata::Metadata;
use rcgen::{CertificateParams, KeyPair, SanType};
use serde::Serialize;
use std::{net::IpAddr, sync::Arc};
use tokio::sync::watch;
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig,
    },
    TlsAcceptor,
};
use tower::ServiceExt as _;

// ── 内部 HTTP 服务器状态 ─────────────────────────────────────
struct ServerState {
    folder: String,
    upload_folder: String,
    app: tauri::AppHandle,
}

// ── 上传事件（发送给桌面端）──────────────────────────────────
#[derive(Clone, Serialize)]
pub struct FileReceivedEvent {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Serialize)]
pub struct UploadProgressEvent {
    pub name: String,
    pub received: u64,
    pub total: u64,
}

// ── Tauri 插件状态 ────────────────────────────────────────────
struct TransferHandle {
    shutdown_tx: watch::Sender<bool>,
}

pub struct TransferState(std::sync::Mutex<Option<TransferHandle>>);

impl TransferState {
    pub fn new() -> Self {
        TransferState(std::sync::Mutex::new(None))
    }
}

// ── 命令返回类型 ─────────────────────────────────────────────
#[derive(Serialize)]
pub struct StartTransferResult {
    pub url: String,
    pub qr_svg: String,
}

// ── Tauri 命令 ───────────────────────────────────────────────

#[tauri::command]
pub async fn start_transfer(
    state: tauri::State<'_, TransferState>,
    app: tauri::AppHandle,
    folder: String,
    upload_folder: String,
    port: u16,
) -> Result<StartTransferResult, String> {
    // 关闭已有服务
    {
        let mut guard = state.0.lock().unwrap();
        if let Some(handle) = guard.take() {
            let _ = handle.shutdown_tx.send(true);
        }
    }

    // 至少需要一个文件夹
    if folder.is_empty() && upload_folder.is_empty() {
        return Err("请至少选择一个文件夹（照片文件夹或接收文件夹）".to_string());
    }

    // 获取本机局域网 IP
    let ip = local_ip_address::local_ip()
        .map_err(|e| format!("无法获取本机 IP: {e}"))?;
    let url = format!("https://{}:{}", ip, port);

    // 生成自签名证书（SAN = LAN IP，有效期 400 天，满足 iOS ≤ 825 天要求）
    let acceptor = {
        let mut params = CertificateParams::default();
        // iOS 13+ 要求有效期 ≤ 825 天；rcgen 默认有效期到 4096 年会导致 Safari 直接拒绝
        params.not_before = time::OffsetDateTime::now_utc();
        params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(400);
        match ip {
            IpAddr::V4(v4) => params.subject_alt_names.push(SanType::IpAddress(IpAddr::V4(v4))),
            IpAddr::V6(v6) => params.subject_alt_names.push(SanType::IpAddress(IpAddr::V6(v6))),
        }
        let key_pair = KeyPair::generate()
            .map_err(|e| format!("密钥生成失败: {e}"))?;
        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| format!("证书生成失败: {e}"))?;
        let cert_der = CertificateDer::from(cert.der().as_ref().to_vec());
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));
        let mut tls_cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .map_err(|e| format!("TLS 配置失败: {e}"))?;
        // 只声明 http/1.1；不加 h2 是因为 hyper-util server-auto 不含 http2，加了反而报无效响应
        tls_cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
        TlsAcceptor::from(Arc::new(tls_cfg))
    };

    // 生成二维码 SVG
    let qr_svg = {
        use qrcode::render::svg;
        use qrcode::QrCode;
        let code =
            QrCode::new(url.as_bytes()).map_err(|e| format!("二维码生成失败: {e}"))?;
        code.render::<svg::Color<'_>>()
            .min_dimensions(200, 200)
            .build()
    };

    // 绑定端口
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .map_err(|e| format!("端口 {} 绑定失败: {}", port, e))?;

    // 构建 axum 路由
    let upload_dir = if upload_folder.is_empty() { folder.clone() } else { upload_folder };
    let srv_state = Arc::new(ServerState {
        folder: folder.clone(),
        upload_folder: upload_dir,
        app,
    });
    let app_router = Router::new()
        .route("/", get(handle_ui))
        .route("/api/list", get(handle_list))
        .route("/api/upload", post(handle_upload))
        .route("/thumb/:name", get(handle_thumb))
        .route("/file/:name", get(handle_file))
        .layer(DefaultBodyLimit::max(200 * 1024 * 1024)) // 200MB，支持高清照片/视频
        .with_state(srv_state);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // 在 Tauri Tokio 运行时中后台运行 HTTPS 服务
    tauri::async_runtime::spawn(async move {
        let mut rx = shutdown_rx;
        loop {
            tokio::select! {
                _ = rx.changed() => {
                    if *rx.borrow() { break; }
                }
                result = listener.accept() => {
                    let Ok((tcp, _)) = result else { break };
                    let tls_acc = acceptor.clone();
                    let router = app_router.clone();
                    tokio::spawn(async move {
                        let Ok(tls) = tls_acc.accept(tcp).await else { return };
                        let io = TokioIo::new(tls);
                        let svc = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                            router.clone().oneshot(req.map(Body::new))
                        });
                        let _ = http1::Builder::new()
                            .keep_alive(true)
                            .serve_connection(io, svc)
                            .await;
                    });
                }
            }
        }
    });

    // 保存 handle
    {
        let mut guard = state.0.lock().unwrap();
        *guard = Some(TransferHandle { shutdown_tx });
    }

    Ok(StartTransferResult { url, qr_svg })
}

#[tauri::command]
pub fn stop_transfer(state: tauri::State<'_, TransferState>) {
    let mut guard = state.0.lock().unwrap();
    if let Some(handle) = guard.take() {
        let _ = handle.shutdown_tx.send(true);
    }
}

#[tauri::command]
pub fn get_transfer_status(state: tauri::State<'_, TransferState>) -> bool {
    state.0.lock().unwrap().is_some()
}

// ── HTTP 处理函数 ─────────────────────────────────────────────

async fn handle_ui() -> Html<&'static str> {
    Html(include_str!("../assets/transfer_ui.html"))
}

/// POST /api/upload — 接收手机上传的文件，保存到 upload_folder
async fn handle_upload(
    State(srv): State<Arc<ServerState>>,
    headers: axum::http::HeaderMap,
    mut multipart: Multipart,
) -> Response {
    use tauri::Emitter as _;
    use tokio::io::AsyncWriteExt as _;

    let total_bytes: u64 = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut saved: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    while let Ok(Some(mut field)) = multipart.next_field().await {
        // 优先用 filename，fallback 到 field name
        let raw_name = field
            .file_name()
            .map(|s| s.to_string())
            .or_else(|| field.name().map(|s| s.to_string()))
            .unwrap_or_else(|| "upload".to_string());

        // 防目录穿越：只保留最后一段文件名
        let base_name = std::path::Path::new(&raw_name)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".to_string());

        if base_name.contains("..") {
            errors.push(format!("{}: 文件名非法", raw_name));
            continue;
        }

        // 处理文件名冲突：追加 _1, _2, ...
        let dest_path = {
            let base = std::path::Path::new(&srv.upload_folder).join(&base_name);
            if !base.exists() {
                base
            } else {
                let stem = std::path::Path::new(&base_name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| base_name.clone());
                let ext = std::path::Path::new(&base_name)
                    .extension()
                    .map(|s| format!(".{}", s.to_string_lossy()))
                    .unwrap_or_default();
                let mut counter = 1u32;
                loop {
                    let candidate = std::path::Path::new(&srv.upload_folder)
                        .join(format!("{}_{}{}", stem, counter, ext));
                    if !candidate.exists() {
                        break candidate;
                    }
                    counter += 1;
                }
            }
        };

        // ── 逐块写文件 + 进度事件 ──────────────────────────────
        let mut file = match tokio::fs::File::create(&dest_path).await {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("{}: 创建文件失败 {}", base_name, e));
                continue;
            }
        };
        let mut received: u64 = 0;
        let mut write_error: Option<String> = None;
        const EMIT_INTERVAL: u64 = 256 * 1024;
        let mut last_emit: u64 = 0;

        while let Ok(Some(chunk)) = field.chunk().await {
            received += chunk.len() as u64;
            if let Err(e) = file.write_all(&chunk).await {
                write_error = Some(format!("{}: 写入失败 {}", base_name, e));
                break;
            }
            if received - last_emit >= EMIT_INTERVAL {
                last_emit = received;
                let _ = srv.app.emit(
                    "transfer://upload-progress",
                    UploadProgressEvent { name: base_name.clone(), received, total: total_bytes },
                );
            }
        }
        let _ = file.flush().await;
        drop(file);

        if let Some(err) = write_error {
            errors.push(err);
            let _ = tokio::fs::remove_file(&dest_path).await;
            continue;
        }

        let final_name = dest_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| base_name.clone());

        let size = tokio::fs::metadata(&dest_path).await.map(|m| m.len()).unwrap_or(received);
        let _ = srv.app.emit(
            "transfer://file-received",
            FileReceivedEvent { name: final_name.clone(), size },
        );
        saved.push(final_name);
    }

    let body = serde_json::json!({ "saved": saved, "errors": errors }).to_string();
    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Body::from(body))
        .unwrap()
}

async fn handle_list(State(srv): State<Arc<ServerState>>) -> Response {
    // 未设置下载文件夹时返回空列表（仅使用上传功能时）
    if srv.folder.is_empty() {
        return HttpResponse::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
            .body(axum::body::Body::from("[]"))
            .unwrap();
    }
    let image_exts = [
        ".jpg", ".jpeg", ".png", ".webp", ".bmp",
        ".tif", ".tiff", ".hif", ".heic", ".heif", ".avif",
    ];
    let dir = match std::fs::read_dir(&srv.folder) {
        Ok(d) => d,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "无法读取文件夹").into_response();
        }
    };
    let mut files: Vec<serde_json::Value> = Vec::new();
    for entry in dir.flatten() {
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ext = std::path::Path::new(&name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();
        if !image_exts.contains(&ext.as_str()) {
            continue;
        }
        files.push(serde_json::json!({ "name": name, "size": meta.len() }));
    }
    files.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    let json = serde_json::to_string(&files).unwrap_or_else(|_| "[]".to_string());
    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(axum::body::Body::from(json))
        .unwrap()
}

async fn handle_thumb(
    State(srv): State<Arc<ServerState>>,
    Path(name): Path<String>,
) -> Response {
    if let Some(resp) = sanitize_name(&name) {
        return resp;
    }
    let path = std::path::Path::new(&srv.folder).join(&name);
    if !path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    let img_result = load_image(&path, &ext);
    let img = match img_result {
        Ok(i) => i,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let img = apply_exif_orientation(img, &path);
    let thumb = img.thumbnail(480, 480);
    let mut buf = Vec::new();
    if thumb
        .write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Jpeg)
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(axum::body::Body::from(buf))
        .unwrap()
}

async fn handle_file(
    State(srv): State<Arc<ServerState>>,
    Path(name): Path<String>,
) -> Response {
    if let Some(resp) = sanitize_name(&name) {
        return resp;
    }
    let path = std::path::Path::new(&srv.folder).join(&name);
    if !path.exists() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = ext_to_mime(ext.as_ref());
    let cd = format!("inline; filename=\"{}\"", name);

    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_DISPOSITION, cd)
        .body(axum::body::Body::from(data))
        .unwrap()
}

// ── 工具函数 ─────────────────────────────────────────────────

/// 拒绝包含路径分隔符或 `..` 的文件名（防目录穿越）
fn sanitize_name(name: &str) -> Option<Response> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        Some(StatusCode::BAD_REQUEST.into_response())
    } else {
        None
    }
}

/// 加载图片：Windows 下对 HEIF 系列使用 WIC，其他用 image crate
fn apply_exif_orientation(img: image::DynamicImage, path: &std::path::Path) -> image::DynamicImage {
    let orientation = Metadata::new_from_path(path)
        .ok()
        .and_then(|meta| {
            meta.get_tag(&ExifTag::Orientation(vec![]))
                .and_then(|tag| {
                    if let ExifTag::Orientation(vals) = tag {
                        vals.first().copied()
                    } else {
                        None
                    }
                })
        })
        .unwrap_or(1);

    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

fn load_image(
    path: &std::path::Path,
    ext: &str,
) -> Result<image::DynamicImage, String> {
    #[cfg(windows)]
    if crate::is_heif_ext(ext) {
        return crate::decode_heif_via_wic(path)
            .map(image::DynamicImage::ImageRgb8);
    }
    let _ = ext; // suppress unused-var on non-Windows
    image::open(path).map_err(|e| e.to_string())
}

fn ext_to_mime(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" | "heif" | "hif" => "image/heic",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
}
