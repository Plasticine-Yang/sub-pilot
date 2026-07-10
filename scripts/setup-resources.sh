#!/usr/bin/env bash
#
# Downloads the bundled external dependencies SubtitleFlow needs to run:
#   - ffmpeg  : static binary for the host platform -> resources/ffmpeg/ffmpeg[.exe]
#   - base model : OpenAI Whisper "base" PyTorch model -> resources/models/base.pt
#   - CJK font : Noto Sans CJK SC for burn-in         -> resources/fonts/NotoSansCJKsc-Regular.otf
#   - whisper : dev venv or Windows bundled runtime
#       dev path      -> resources/whisper/venv
#       Windows bundle -> resources/whisper/windows/whisper/whisper.exe
#
# These artifacts are git-ignored (see .gitignore) because they are large
# binaries. Run this script once after cloning so `npm run tauri dev` and the
# first-launch self-check succeed.
#
# Idempotent: files with a matching SHA-256 are left untouched.
#
# Environment toggles (used by CI):
#   SKIP_WHISPER_VENV=1  skip provisioning the ~900MB dev whisper venv. The bundle
#                        resources (ffmpeg/model/font) are all `tauri build`
#                        needs to compile; the venv is only for real dev-time
#                        transcription, which CI does not exercise.
#   BUNDLE_WHISPER_RUNTIME=1
#                        build a self-contained Windows Whisper runtime with
#                        PyInstaller. Used by the Windows release job.
#   FFMPEG_PLATFORM=...   override host detection: darwin-arm64 | darwin-x64 |
#                        linux-x64 | win32-x64.

set -euo pipefail

# --- Resolve repo paths (script lives in <repo>/scripts) --------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FFMPEG_DIR="${REPO_ROOT}/resources/ffmpeg"
MODELS_DIR="${REPO_ROOT}/resources/models"
FONTS_DIR="${REPO_ROOT}/resources/fonts"
WHISPER_DIR="${REPO_ROOT}/resources/whisper"
WHISPER_WINDOWS_DIR="${WHISPER_DIR}/windows"
WHISPER_WINDOWS_APP_DIR="${WHISPER_WINDOWS_DIR}/whisper"
WHISPER_WINDOWS_EXE="${WHISPER_WINDOWS_APP_DIR}/whisper.exe"

# --- Detect host platform for the ffmpeg asset ------------------------------
# ffmpeg-static publishes one static binary per platform in a single release.
detect_platform() {
  if [[ -n "${FFMPEG_PLATFORM:-}" ]]; then
    echo "${FFMPEG_PLATFORM}"
    return
  fi
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}" in
    Darwin) case "${arch}" in
        arm64) echo "darwin-arm64" ;;
        x86_64) echo "darwin-x64" ;;
        *) echo "unsupported-darwin-${arch}" ;;
      esac ;;
    Linux) echo "linux-x64" ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT) echo "win32-x64" ;;
    *) echo "unsupported-${os}-${arch}" ;;
  esac
}

PLATFORM="$(detect_platform)"

# --- Pinned artifacts (URL + expected SHA-256) -----------------------------
# ffmpeg 6.0 static binaries from eugeneware/ffmpeg-static (release b6.1.1),
# one per platform. All four SHAs are pinned; the host's is selected below.
#
# Non-Windows builds use `resources/ffmpeg/ffmpeg`; Windows release builds use
# `resources/ffmpeg/ffmpeg.exe` via `tauri.windows.conf.json`.
FFMPEG_RELEASE="https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1"
case "${PLATFORM}" in
  darwin-arm64)
    FFMPEG_URL="${FFMPEG_RELEASE}/ffmpeg-darwin-arm64"
    FFMPEG_SHA256="a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584" ;;
  darwin-x64)
    FFMPEG_URL="${FFMPEG_RELEASE}/ffmpeg-darwin-x64"
    FFMPEG_SHA256="ebdddc936f61e14049a2d4b549a412b8a40deeff6540e58a9f2a2da9e6b18894" ;;
  linux-x64)
    FFMPEG_URL="${FFMPEG_RELEASE}/ffmpeg-linux-x64"
    FFMPEG_SHA256="e7e7fb30477f717e6f55f9180a70386c62677ef8a4d4d1a5d948f4098aa3eb99" ;;
  win32-x64)
    FFMPEG_URL="${FFMPEG_RELEASE}/ffmpeg-win32-x64"
    FFMPEG_SHA256="04e1307997530f9cf2fe35cba2ca7e8875ca91da02f89d6c7243df819c94ad00" ;;
  *)
    echo "✗ unsupported platform '${PLATFORM}'. Set FFMPEG_PLATFORM to one of: darwin-arm64, darwin-x64, linux-x64, win32-x64." >&2
    exit 1 ;;
esac
if [[ "${PLATFORM}" == "win32-x64" ]]; then
  FFMPEG_OUT="${FFMPEG_DIR}/ffmpeg.exe"
else
  FFMPEG_OUT="${FFMPEG_DIR}/ffmpeg"
fi

# OpenAI Whisper "base" PyTorch model (official checksum embedded in URL path).
MODEL_URL="https://openaipublic.azureedge.net/main/whisper/models/ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e/base.pt"
MODEL_SHA256="ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e"
MODEL_OUT="${MODELS_DIR}/base.pt"

# Noto Sans CJK SC (Regular) — bundled so burn-in renders 中文/日文 glyphs.
FONT_URL="https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf"
FONT_SHA256="2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b"
FONT_OUT="${FONTS_DIR}/NotoSansCJKsc-Regular.otf"

# --- Helpers ----------------------------------------------------------------
sha256_of() {
  # macOS ships `shasum`; Linux CI may only have `sha256sum`.
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

fetch() {
  # fetch <url> <output> <expected_sha256> <mode>
  local url="$1" out="$2" want="$3" mode="$4"
  if [[ -f "${out}" ]]; then
    local have
    have="$(sha256_of "${out}")"
    if [[ "${have}" == "${want}" ]]; then
      echo "✓ $(basename "${out}") already present and verified"
      return 0
    fi
    echo "! $(basename "${out}") checksum mismatch, re-downloading"
    rm -f "${out}"
  fi

  echo "↓ downloading $(basename "${out}") …"
  mkdir -p "$(dirname "${out}")"
  curl -fSL --retry 3 -o "${out}" "${url}"

  local got
  got="$(sha256_of "${out}")"
  if [[ "${got}" != "${want}" ]]; then
    echo "✗ checksum FAILED for $(basename "${out}")" >&2
    echo "    expected ${want}" >&2
    echo "    got      ${got}" >&2
    rm -f "${out}"
    exit 1
  fi
  chmod "${mode}" "${out}"
  echo "✓ $(basename "${out}") downloaded and verified"
}

python_cmd() {
  if [[ -n "${PYTHON:-}" ]]; then
    echo "${PYTHON}"
    return 0
  fi
  if command -v python3 >/dev/null 2>&1; then
    command -v python3
    return 0
  fi
  if command -v python >/dev/null 2>&1; then
    command -v python
    return 0
  fi
  return 1
}

venv_python() {
  local venv="$1"
  if [[ "${PLATFORM}" == "win32-x64" ]]; then
    echo "${venv}/Scripts/python.exe"
  else
    echo "${venv}/bin/python"
  fi
}

venv_whisper() {
  local venv="$1"
  if [[ "${PLATFORM}" == "win32-x64" ]]; then
    echo "${venv}/Scripts/whisper.exe"
  else
    echo "${venv}/bin/whisper"
  fi
}

# --- whisper runtime ---------------------------------------------------------
# Non-Windows dev builds spawn resources/whisper/whisper (a tracked launcher)
# which forwards to a co-located venv. Windows release builds spawn the
# PyInstaller executable under resources/whisper/windows/whisper/.
provision_whisper() {
  if [[ "${SKIP_WHISPER_VENV:-0}" == "1" ]]; then
    echo "• SKIP_WHISPER_VENV=1 — skipping whisper venv provisioning"
    return 0
  fi
  local venv="${WHISPER_DIR}/venv"
  local whisper_bin
  whisper_bin="$(venv_whisper "${venv}")"
  if [[ -x "${whisper_bin}" ]]; then
    echo "✓ whisper venv already present"
    return 0
  fi
  local python
  if ! python="$(python_cmd)"; then
    echo "✗ python3 not found — install Python 3 to provision the whisper runtime" >&2
    exit 1
  fi
  local venv_py
  venv_py="$(venv_python "${venv}")"
  echo "↓ creating whisper venv and installing openai-whisper (pulls PyTorch) …"
  "${python}" -m venv "${venv}"
  "${venv_py}" -m pip install --quiet --upgrade pip
  "${venv_py}" -m pip install --quiet openai-whisper
  echo "✓ whisper venv ready"
}

build_windows_whisper_runtime() {
  if [[ "${BUNDLE_WHISPER_RUNTIME:-0}" != "1" ]]; then
    return 0
  fi
  if [[ "${PLATFORM}" != "win32-x64" ]]; then
    echo "✗ BUNDLE_WHISPER_RUNTIME=1 is only supported for FFMPEG_PLATFORM=win32-x64" >&2
    exit 1
  fi

  local python
  if ! python="$(python_cmd)"; then
    echo "✗ python not found — install Python 3.11+ to build the Windows whisper runtime" >&2
    exit 1
  fi

  local build_venv="${WHISPER_DIR}/build-venv"
  local build_python
  build_python="$(venv_python "${build_venv}")"
  local pyinstaller_work="${REPO_ROOT}/target/pyinstaller"

  echo "↓ building bundled Windows Whisper runtime with PyInstaller …"
  rm -rf "${build_venv}" "${WHISPER_WINDOWS_DIR}" "${pyinstaller_work}"
  "${python}" -m venv "${build_venv}"
  "${build_python}" -m pip install --quiet --upgrade pip wheel setuptools
  "${build_python}" -m pip install --quiet pyinstaller openai-whisper
  mkdir -p "${WHISPER_WINDOWS_DIR}" "${pyinstaller_work}"

  "${build_python}" -m PyInstaller \
    --noconfirm \
    --clean \
    --onedir \
    --name whisper \
    --distpath "${WHISPER_WINDOWS_DIR}" \
    --workpath "${pyinstaller_work}" \
    --specpath "${pyinstaller_work}" \
    --collect-all whisper \
    --collect-all torch \
    --collect-all tiktoken \
    --collect-submodules tiktoken_ext \
    --copy-metadata openai-whisper \
    --copy-metadata tiktoken \
    --copy-metadata torch \
    --copy-metadata numba \
    --copy-metadata numpy \
    "${SCRIPT_DIR}/whisper-entry.py"

  if [[ ! -f "${WHISPER_WINDOWS_EXE}" ]]; then
    echo "✗ PyInstaller did not create ${WHISPER_WINDOWS_EXE}" >&2
    exit 1
  fi
  "${WHISPER_WINDOWS_EXE}" --help >/dev/null
  rm -rf "${build_venv}" "${pyinstaller_work}"
  echo "✓ Windows whisper runtime ready at ${WHISPER_WINDOWS_APP_DIR}"
}

# --- Run --------------------------------------------------------------------
echo "SubtitleFlow — fetching bundled resources into resources/ (platform: ${PLATFORM})"
fetch "${FFMPEG_URL}" "${FFMPEG_OUT}" "${FFMPEG_SHA256}" "755"
fetch "${MODEL_URL}"  "${MODEL_OUT}"  "${MODEL_SHA256}"  "644"
fetch "${FONT_URL}"   "${FONT_OUT}"   "${FONT_SHA256}"   "644"
if [[ "${PLATFORM}" == "win32-x64" ]]; then
  mkdir -p "${WHISPER_WINDOWS_APP_DIR}"
fi
# The launcher shim is a bash script (dev-only, POSIX shells); chmod is a no-op
# on Windows checkouts but harmless.
chmod 755 "${WHISPER_DIR}/whisper" 2>/dev/null || true
if [[ "${BUNDLE_WHISPER_RUNTIME:-0}" == "1" ]]; then
  build_windows_whisper_runtime
else
  provision_whisper
fi
echo "Done. Resources are ready under resources/."
