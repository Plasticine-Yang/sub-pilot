import { readFileSync } from "node:fs";

function fail(message) {
  console.error(`windows-release-runtime: ${message}`);
  process.exit(1);
}

function read(path) {
  return readFileSync(new URL(`../../${path}`, import.meta.url), "utf8");
}

const windowsConfigPath = "src-tauri/tauri.windows.conf.json";
let windowsConfig;
try {
  windowsConfig = JSON.parse(read(windowsConfigPath));
} catch (error) {
  fail(`cannot read ${windowsConfigPath}: ${error}`);
}

const resources = windowsConfig.bundle?.resources;
const ffmpegTarget = resources?.["../resources/ffmpeg/ffmpeg.exe"];
if (ffmpegTarget !== "resources/ffmpeg/ffmpeg.exe") {
  fail(
    `${windowsConfigPath} must bundle ../resources/ffmpeg/ffmpeg.exe to resources/ffmpeg/ffmpeg.exe`,
  );
}

const whisperRuntimeTarget =
  resources?.["../resources/whisper/windows/whisper"];
if (whisperRuntimeTarget !== "resources/whisper/windows/whisper") {
  fail(
    `${windowsConfigPath} must bundle ../resources/whisper/windows/whisper to resources/whisper/windows/whisper`,
  );
}

const setupResources = read("scripts/setup-resources.sh");
if (!setupResources.includes("BUNDLE_WHISPER_RUNTIME")) {
  fail("scripts/setup-resources.sh must expose BUNDLE_WHISPER_RUNTIME");
}
if (!setupResources.includes("pyinstaller")) {
  fail("scripts/setup-resources.sh must build a self-contained Windows runtime with PyInstaller");
}

const releaseWorkflow = read(".github/workflows/release.yml");
if (!releaseWorkflow.includes("actions/setup-python")) {
  fail("release.yml must install Python before building the Windows Whisper runtime");
}
if (!releaseWorkflow.includes("BUNDLE_WHISPER_RUNTIME")) {
  fail("release.yml must enable BUNDLE_WHISPER_RUNTIME for the Windows leg");
}
if (releaseWorkflow.includes("已知限制：本版尚未内置 Whisper 运行时")) {
  fail("release notes must not claim the Windows build lacks Whisper runtime");
}

console.log("windows-release-runtime: ok");
