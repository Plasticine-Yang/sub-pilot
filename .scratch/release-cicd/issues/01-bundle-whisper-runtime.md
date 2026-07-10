# 打包 Whisper 运行时随应用分发（终端用户可转写）

Status: ready-for-human

## Progress

- 2026-07-11：Windows release 路径已改为在 CI 中用 PyInstaller 构建自包含 OpenAI Whisper runtime，并通过 `tauri.windows.conf.json` 随安装包打入 `resources/whisper/windows/whisper/`。macOS/Linux 的随包 runtime 仍待本 issue 后续覆盖。

## 背景

ADR-0005 决定转写引擎用 OpenAI 官方原版 whisper（Python + PyTorch），并明确「把 Python 运行时随应用打包」，但该打包被延后（deferred）。当前状态：

- `resources/whisper/whisper` 是一个 **bash launcher shim**，把参数转发给本机已安装的 whisper CLI（开发路径）。
- 真正的运行时是本地 `resources/whisper/venv`（约 874MB 的 PyTorch venv），**既不入库，也不进安装包**（见 `.gitignore` 与 `tauri.conf.json` 的 `bundle.resources`——只打包了 ffmpeg/model/font）。

**后果**：CI 产出的三平台安装包里没有可用的 whisper 运行时，终端用户装上后转写会直接失败（自检里 whisper 组件会报缺失）。这是「产物能否真正发给用户」的关键卡点，与 CI 链路本身无关。

## What to build

把一个可离线运行的 whisper 运行时随应用打包进 `resources/`，让终端用户开箱即可转写，替换掉现在的 dev-only bash launcher。

- 选定打包方案（候选：PyInstaller 冻结 whisper CLI / 嵌入式 Python + 预装 wheel / 其它），需权衡体积、启动速度、跨平台可行性。
- 三平台各自产出可执行的运行时（macOS arm64+x64、Windows、Linux），入口与 `src-tauri/src/whisper.rs` 现有的 `Command::new(whisper)` 调用约定兼容（`<audio> --model <m> --model_dir <dir> --output_format srt --verbose True --output_dir <dir>`）。
- 更新 `tauri.conf.json` 的 `bundle.resources` 纳入该运行时。
- 更新 `scripts/setup-resources.sh` 的 dev 路径（或保留 launcher 作为 dev 回退）。
- 更新首次启动自检 `src-tauri/src/selfcheck.rs`：确认打包运行时可执行。
- 更新 Release workflow 与 README，移除「转写不可用」的已知限制说明。

## 验收

三平台的 CI 产物安装后，导入一个短视频能真实转写出 `original.srt`（用 `resources/whisper/fixture_audio.wav` 或等价 fixture 做端到端验证）。

## 相关

- ADR-0005（转写引擎）、ADR-0002（依赖交付策略）
- `.github/workflows/release.yml`（当前 releaseBody 里标注了此限制）
- `scripts/setup-resources.sh`、`src-tauri/src/whisper.rs`、`src-tauri/src/selfcheck.rs`
