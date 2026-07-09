# Windows / Linux 资源适配（ffmpeg 路径与 launcher）

Status: ready-for-human

## 背景

项目最初只面向 macOS（Apple Silicon）首发。为了让三平台 Release workflow 能构建通过，已做了最小适配，但 Windows/Linux 上的运行时行为尚未打通：

- `scripts/setup-resources.sh` 现在按 host 平台选 ffmpeg 二进制（darwin-arm64/x64、linux-x64、win32-x64），但**统一写到 `resources/ffmpeg/ffmpeg`（无扩展名）**。这是因为 `tauri.conf.json` 只打包这一个路径，且 `tauri-build` 编译期会校验它存在。在 Windows 上这个文件是一个「没有 .exe 后缀的 PE 可执行文件」——够编译和打包，但：
  - `src-tauri/src/media.rs` / `whisper.rs` 里 `Command::new` 拼的路径是否能在 Windows 正确 spawn，未验证。
  - `resources/whisper/whisper` 是 **bash 脚本**，Windows 无 bash 时无法执行。
- `.gitignore` 的资源忽略规则、自检 `selfcheck.rs` 的 `probe_executable` 在非 Unix 下的可执行位判断，都是按 macOS 假设写的。

## What to build

让 Windows 与 Linux 上的 ffmpeg 调用与（配合 issue 01 的）whisper 运行时都能真实工作。

- 决定 Windows 下 ffmpeg 的落地形态（`.exe` 后缀 + `tauri.conf.json` 按平台配置 resources，或保持无后缀并验证 spawn 可行）。
- Windows 下替换 bash launcher（issue 01 的打包运行时应当天然解决，否则需 `.cmd`/原生入口）。
- 校验 `selfcheck.rs` 在 Windows/Linux 的可执行探测逻辑。
- 三平台各跑一遍导入→（转写）→导出，确认 ffmpeg 抽音频/mux/烧录都正常。

## 验收

Linux 与 Windows 的 CI 产物安装后，ffmpeg 相关路径（导入探测时长、sidecar/软字幕/烧录导出）全部正常。

## 相关

- `scripts/setup-resources.sh`、`src-tauri/tauri.conf.json`
- `src-tauri/src/media.rs`、`src-tauri/src/whisper.rs`、`src-tauri/src/selfcheck.rs`
- 依赖 issue 01（whisper 运行时打包）一并解决 launcher 问题
