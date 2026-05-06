use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, Response as HttpResponse, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use hyper::{body::Incoming, server::conn::http1};
use hyper_util::rt::TokioIo;
use image::ImageFormat;
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
    folder: String,
    port: u16,
) -> Result<StartTransferResult, String> {
    // 关闭已有服务
    {
        let mut guard = state.0.lock().unwrap();
        if let Some(handle) = guard.take() {
            let _ = handle.shutdown_tx.send(true);
        }
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
    let srv_state = Arc::new(ServerState { folder: folder.clone() });
    let app = Router::new()
        .route("/", get(handle_ui))
        .route("/api/list", get(handle_list))
        .route("/thumb/:name", get(handle_thumb))
        .route("/file/:name", get(handle_file))
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
                    let router = app.clone();
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

async fn handle_list(State(srv): State<Arc<ServerState>>) -> Response {
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
