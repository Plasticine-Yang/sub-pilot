# SubtitleFlow

一款完全本地运行、免费的 AI 视频字幕工具（Tauri v2 桌面版，macOS 首发）。

导入视频 → 本地 Whisper 转写出原始字幕 → 生成翻译 Prompt 供外部 AI 翻译 → 导回译文并校验 → 导出外挂字幕或烧录进画面。全程离线、不登录、不上传。

产品愿景见 `SubtitleFlow_PRD.md`；术语见 `CONTEXT.md`；架构决策见 `docs/adr/`。

## 技术栈

- 前端：Tauri v2 + React + TypeScript + TailwindCSS + shadcn/ui
- 后端：Rust（转写走 OpenAI 官方 whisper（Python + PyTorch，随包），媒体处理走 ffmpeg）

## 开发环境准备

需要 Node.js、Rust 工具链（rustup）。

```bash
# 1. 安装前端依赖
npm install

# 2. 拉取随包资源（ffmpeg 静态二进制 + Whisper base.pt 模型 + whisper 运行时）
#    大文件不入库，由脚本按校验和下载到 resources/ 下；并在
#    resources/whisper/venv 里装好 openai-whisper（需本机有 python3）
./scripts/setup-resources.sh

# 3. 启动桌面应用（开发模式）
npm run tauri dev
```

首次启动时应用会自检 `resources/ffmpeg/ffmpeg` 可执行、`resources/models/base.pt` 存在、`resources/whisper/whisper` 可执行；缺失时首页会给出明确提示。

## 常用脚本

```bash
npm run typecheck   # tsc --noEmit
npm run lint        # eslint
cargo test          # 后端单测（在 src-tauri/ 下运行）
```
