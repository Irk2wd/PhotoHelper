# PhotoHelper

基于 Tauri v2 + React + TypeScript 构建的个人照片管理桌面应用。

当前功能：**HEIF / RAW 同步** —— 扫描文件夹，自动配对同名照片，按选定方向将孤立文件移入回收站。

---

## 从源码打包

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

---

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
src-tauri/target/release/tauri-app.exe          # 免安装可执行文件
src-tauri/target/release/bundle/msi/            # Windows MSI 安装包
```

> 首次构建需要下载并编译 Rust 依赖，耗时约 5–15 分钟，后续增量编译约 30 秒。
