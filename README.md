# SubtitleFlow

一款完全本地运行、免费的 AI 视频字幕工具（Tauri v2 桌面版，macOS 首发）。

导入视频 → 本地 Whisper 转写出原始字幕 → 生成翻译 Prompt 供外部 AI 翻译 → 导回译文并校验 → 导出外挂字幕或烧录进画面。全程离线、不登录、不上传。

产品愿景见 `SubtitleFlow_PRD.md`；术语见 `CONTEXT.md`；架构决策见 `docs/adr/`。

## 下载与安装

从 [GitHub Releases](https://github.com/Plasticine-Yang/sub-pilot/releases) 下载对应平台的安装包（macOS `.dmg`、Windows `.msi`/`.exe`、Linux `.AppImage`/`.deb`）。

> **当前为未签名构建**，首次启动会被系统安全机制拦截，需手动放行：
>
> - **macOS**：提示「已损坏」或「无法验证开发者」时，在「系统设置 → 隐私与安全性」点「仍要打开」，或终端执行 `xattr -dr com.apple.quarantine /Applications/SubtitleFlow.app`。
> - **Windows**：SmartScreen 弹「Windows 已保护你的电脑」时，点「更多信息 → 仍要运行」。
>
> 代码签名与公证正在跟进（见 `.scratch/release-cicd/issues/03-code-signing-and-notarization.md`）。

> **平台状态**：Windows 安装包已内置 Whisper 运行时（Python + PyTorch），安装后可直接本地转写；macOS/Linux 的 Whisper 随包运行时仍在跟进，当前主要用于导入、界面与导出路径验证。

## 技术栈

- 前端：Tauri v2 + React + TypeScript + TailwindCSS + shadcn/ui
- 后端：Rust（转写走 OpenAI 官方 whisper（Python + PyTorch，随包），媒体处理走 ffmpeg）

## 开发环境准备

需要 Node.js、Rust 工具链（rustup）。

```bash
# 1. 安装前端依赖
npm install

# 2. 拉取随包资源（ffmpeg 静态二进制 + Whisper base.pt 模型 + whisper 运行时）
#    大文件不入库，由脚本按 host 平台与校验和下载到 resources/ 下；并在
#    resources/whisper/venv 里装好 openai-whisper（需本机有 python3）
./scripts/setup-resources.sh

# 3. 启动桌面应用（开发模式）
npm run tauri dev
```

首次启动时应用会自检 `resources/ffmpeg/ffmpeg` 可执行、`resources/models/base.pt` 存在，并单独检查 Whisper 运行时。Windows 安装包应当自带运行时并可直接转写；缺失时首页会给出明确提示。

## 常用脚本

```bash
npm run typecheck   # tsc --noEmit
npm run lint        # eslint
cargo test          # 后端单测（在 src-tauri/ 下运行）
```

## CI / 发布

- **CI**（`.github/workflows/ci.yml`）：每次 push / PR 到 `main` 触发，跑前端 `typecheck` + `lint` 与后端 `cargo test`。
- **发布**（`.github/workflows/release.yml`）：push 一个 `v*` 版本 tag 触发，在 macOS（Apple Silicon + Intel）、Windows、Linux 上分别构建安装包，并创建一个 **草稿（draft）** GitHub Release。

发布一版：

```bash
# 确认 main 已绿（CI 通过），版本号与 package.json / tauri.conf.json / Cargo.toml 一致
git tag v0.1.0
git push origin v0.1.0
```

workflow 跑完后，到 GitHub Releases 页面检查产物与发布说明，确认无误再手动 **Publish** 草稿。

> 产物目前**未签名**。Windows 安装包已内置 Whisper 运行时；macOS/Linux 的随包运行时、剩余平台资源适配、代码签名/公证仍见 `.scratch/release-cicd/issues/`。
