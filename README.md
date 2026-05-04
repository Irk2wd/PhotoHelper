# PhotoHelper

基于 Tauri v2 + React + TypeScript 构建的个人照片管理桌面应用。

---

## 功能介绍

### HEIF / RAW 同步

相机拍摄时同时输出 HEIF（如 `.hif`）和 RAW（如 `.arw`）文件，二者同名但扩展名不同。随着删选操作的进行，两种格式之间容易出现孤立文件。本功能可自动扫描并配对，按选定方向清理孤立文件（移入回收站，可恢复）。

**文件夹模式**

| 模式 | 说明 |
|------|------|
| 分离模式 | HEIF 和 RAW 分别存放在两个独立文件夹（如 `j/` 和 `r/`） |
| 混合模式 | HEIF 和 RAW 混放在同一个文件夹 |

**同步方向**

| 方向 | 说明 |
|------|------|
| 以 HEIF 为准 | 删除没有对应 HEIF 的孤立 RAW |
| 以 RAW 为准 | 删除没有对应 RAW 的孤立 HEIF |
| 双向同步 | 删除所有孤立文件，只保留成对文件 |

**支持的扩展名**

- HEIF 侧：`.hif` `.heic` `.heif` `.avif`（可多选，默认 `.hif`）
- RAW 侧：`.arw` `.cr3` `.cr2` `.nef` `.raf` `.dng` `.orf` `.rw2`（可多选，默认 `.arw`）

---

### 照片分类

扫描指定文件夹，按文件格式自动创建子文件夹并将文件归类整理，支持预览后再执行。

**分类规则**

| 子文件夹 | 归入的扩展名 |
|---------|------------|
| `RAW/` | `.arw` `.cr3` `.cr2` `.nef` `.raf` `.dng` `.orf` `.rw2` `.raw` 等 |
| `HEIF/` | `.hif` `.heic` `.heif` `.avif` |
| `JPEG/` | `.jpg` `.jpeg` |
| `PNG/` | `.png` |
| `TIFF/` | `.tif` `.tiff` |
| `Video/` | `.mp4` `.mov` `.avi` `.mkv` `.mts` 等 |
| `Other/` | 其余所有格式 |

已存在于子文件夹中的同名文件会被跳过（不覆盖），操作失败的文件会单独列出。

---

### 图片处理（批量 Pipeline）

对指定文件夹中的图片执行一连串可配置操作（Pipeline），支持预览再执行。

**支持的 Pipeline 步骤**

| 步骤 | 说明 |
|------|------|
| 重命名 | 批量按规则重命名：前缀序号、保留原名加后缀、自定义前缀+序号+后缀，可设置起始编号与补零位数 |
| 压缩 | JPEG/WEBP/BMP/TIFF → 指定质量的 JPEG；PNG → 无损重编码或转 JPEG |

**执行时功能**

- 实时进度条：显示「已处理 / 总计」，处理大批文件时保持响应
- **停止按钮**：处理过程中可随时点击「停止」立即中断，已处理的文件不会受影响，未处理的文件跳过

**支持的图片格式**

| 格式 | 处理方式 |
|------|---------|
| `.jpg` `.jpeg` `.png` `.webp` `.bmp` `.tif` `.tiff` | 直接压缩 |
| `.hif` `.heic` `.heif` `.avif` | 通过 Windows WIC API 解码后转 JPEG（需安装 HEIF 扩展，见下方说明） |

**HEIF / HIF 压缩支持**

Windows 系统通过「HEIF 图像扩展」（WIC 编解码器）支持 `.hif` / `.heic` / `.heif` / `.avif` 格式的解码与压缩转换。

- 在 [Microsoft Store](https://www.microsoft.com/store/productId/9PMMSR1CGPWG) 搜索并安装「**HEIF Image Extensions**」（免费）即可启用
- 安装后，HEIF 文件在压缩步骤中会被解码并以指定 JPEG 质量输出为 `.jpg`
- 若未安装扩展，程序会自动回退为直接复制原文件，**不会崩溃或影响其他格式的处理**
- 处理完成后，结果面板会分别显示 WIC 转换数量与跳过压缩数量

处理结果输出到原文件夹的 `output/` 子目录（可自定义），原文件不受影响。

---

## 从源码构建

### 环境要求

| 工具 | 版本要求 | 安装方式 |
|------|----------|----------|
| [Node.js](https://nodejs.org/) | >= 18 | 官网下载或 `winget install OpenJS.NodeJS` |
| [Rust](https://www.rust-lang.org/tools/install) | stable | `winget install Rustlang.Rustup`，安装后重启终端 |
| [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) | 任意 | Windows 11 已内置；Windows 10 需手动安装 |

> Windows 安装 Rust 后若提示找不到 `cargo`，在 PowerShell 执行：
> ```powershell
> $env:PATH += ";$env:USERPROFILE\.cargo\bin"
> ```
> 或永久写入环境变量：
> ```powershell
> [System.Environment]::SetEnvironmentVariable("PATH", $env:PATH + ";$env:USERPROFILE\.cargo\bin", "User")
> ```

### 步骤

**1. 克隆仓库**

```bash
git clone git@github.com:Irk2wd/PhotoHelper.git
cd PhotoHelper
```

**2. 安装前端依赖**

```bash
npm install
```

**3. 开发模式（热更新）**

```bash
npm run tauri dev
```

**4. 构建生产包**

```bash
npm run tauri build
```

构建产物位于：

```
src-tauri/target/release/bundle/msi/PhotoHelper_*_x64_en-US.msi
```

> **注意**：裸 `.exe` 文件需要系统已安装 WebView2 运行时才能运行，不建议直接分发。请使用 MSI 安装包，它会自动处理 WebView2 依赖。

> 首次构建需要下载并编译 Rust 依赖，耗时约 5–15 分钟，后续增量编译约 30 秒。

