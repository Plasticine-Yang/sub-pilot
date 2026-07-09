#!/usr/bin/env bash
#
# Downloads the bundled external dependencies SubtitleFlow needs to run:
#   - ffmpeg  : static macOS/Apple-Silicon binary  -> resources/ffmpeg/ffmpeg
#   - base model : OpenAI Whisper "base" PyTorch model -> resources/models/base.pt
#   - CJK font : Noto Sans CJK SC for burn-in         -> resources/fonts/NotoSansCJKsc-Regular.otf
#
# These artifacts are git-ignored (see .gitignore) because they are large
# binaries. Run this script once after cloning so `npm run tauri dev` and the
# first-launch self-check succeed.
#
# Idempotent: files with a matching SHA-256 are left untouched.

set -euo pipefail

# --- Resolve repo paths (script lives in <repo>/scripts) --------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
FFMPEG_DIR="${REPO_ROOT}/resources/ffmpeg"
MODELS_DIR="${REPO_ROOT}/resources/models"
FONTS_DIR="${REPO_ROOT}/resources/fonts"

# --- Pinned artifacts (URL + expected SHA-256) -----------------------------
# ffmpeg 6.0 static arm64 from eugeneware/ffmpeg-static (release b6.1.1).
FFMPEG_URL="https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64"
FFMPEG_SHA256="a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584"
FFMPEG_OUT="${FFMPEG_DIR}/ffmpeg"

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

# --- Run --------------------------------------------------------------------
echo "SubtitleFlow — fetching bundled resources into resources/"
fetch "${FFMPEG_URL}" "${FFMPEG_OUT}" "${FFMPEG_SHA256}" "755"
fetch "${MODEL_URL}"  "${MODEL_OUT}"  "${MODEL_SHA256}"  "644"
fetch "${FONT_URL}"   "${FONT_OUT}"   "${FONT_SHA256}"   "644"
echo "Done. Resources are ready under resources/."
