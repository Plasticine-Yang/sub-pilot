# 用 whisper.cpp（经 whisper-rs）作为转写引擎，而非 faster-whisper

PRD 初稿设想用 faster-whisper（Python CLI）。我们改为 whisper.cpp，通过 `whisper-rs` 直接链接进 Rust 后端。原因：产品原则要求「完全本地、免安装、易分发」，而 faster-whisper 需把整个 Python 运行时打包进 Tauri 应用，体积大且 macOS 代码签名复杂；whisper.cpp 是纯 C/C++，原生支持 Apple Silicon 的 Metal/CoreML 加速，无 Python 依赖，可编译为随应用分发的单一产物。代价是模型采用 GGML/GGUF 格式（而非 CT2），但对用户透明。
