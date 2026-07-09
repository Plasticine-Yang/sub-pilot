# 代码签名与公证（macOS notarization / Windows code signing）

Status: ready-for-human

## 背景

当前 Release workflow（`.github/workflows/release.yml`）产出的是**未签名**产物：

- **macOS**：未签名 + 未公证的 `.dmg`/`.app` 会被 Gatekeeper 拦截，用户看到「已损坏，应移到废纸篓」或「无法验证开发者」。
- **Windows**：未签名的 `.exe`/`.msi` 会触发 SmartScreen「未知发布者」警告。

已在 README「下载与安装」写明了用户侧的临时绕过方法，但这不是可持续的分发方式。

## What to build

给三平台产物接上代码签名与（macOS）公证。

- **macOS**：Apple Developer ID Application 证书 + notarytool 公证。tauri-action 支持通过环境变量注入：`APPLE_CERTIFICATE`、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_SIGNING_IDENTITY`、`APPLE_ID`、`APPLE_PASSWORD`(app-specific)、`APPLE_TEAM_ID`。
- **Windows**：代码签名证书（OV/EV 或 Azure Trusted Signing），在 `tauri.conf.json` 的 `bundle.windows` 配置证书指纹/时间戳服务器，或在 workflow 里签名。
- 证书与密码存入 GitHub Secrets（**不入库**），在 release workflow 的 build step 注入。
- 更新 README，移除绕过说明。

## 前置

需要**人工提供证书**：Apple Developer 账号（$99/年）与 Windows 代码签名证书。属于 `ready-for-human`——agent 无法代取证书。

## 相关

- `.github/workflows/release.yml`、`src-tauri/tauri.conf.json`
- tauri 分发文档：https://v2.tauri.app/distribute/
