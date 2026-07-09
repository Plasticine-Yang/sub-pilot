use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Manager};

/// State of a single bundled component after probing it on disk.
/// Serialized to the frontend, which owns the user-facing copy.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ComponentState {
    Ok,
    Missing,
    NotExecutable,
}

/// Structured first-launch self-check result handed to the frontend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelfCheckReport {
    pub ok: bool,
    pub ffmpeg: ComponentState,
    pub model: ComponentState,
}

/// Pure mapping from raw probe outcomes to the report the UI consumes.
/// No IO — the caller performs the filesystem/executable probes.
pub fn evaluate(ffmpeg: ComponentState, model: ComponentState) -> SelfCheckReport {
    let ok = ffmpeg == ComponentState::Ok && model == ComponentState::Ok;
    SelfCheckReport { ok, ffmpeg, model }
}

/// Relative paths of the bundled resources, mirroring `tauri.conf.json`.
const FFMPEG_RESOURCE: &str = "resources/ffmpeg/ffmpeg";
const MODEL_RESOURCE: &str = "resources/models/base.pt";

/// Runs the first-launch self-check against the real bundled resources.
pub fn run_self_check(app: &AppHandle) -> SelfCheckReport {
    let resource_dir = app.path().resource_dir().ok();
    let ffmpeg = probe_executable(resource_dir.as_deref(), FFMPEG_RESOURCE);
    let model = probe_file(resource_dir.as_deref(), MODEL_RESOURCE);
    evaluate(ffmpeg, model)
}

fn probe_file(resource_dir: Option<&Path>, rel: &str) -> ComponentState {
    match resource_dir {
        Some(dir) if dir.join(rel).is_file() => ComponentState::Ok,
        _ => ComponentState::Missing,
    }
}

fn probe_executable(resource_dir: Option<&Path>, rel: &str) -> ComponentState {
    let Some(dir) = resource_dir else {
        return ComponentState::Missing;
    };
    let path = dir.join(rel);
    if !path.is_file() {
        return ComponentState::Missing;
    }
    if is_executable(&path) {
        ComponentState::Ok
    } else {
        ComponentState::NotExecutable
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_present_reports_ok() {
        let report = evaluate(ComponentState::Ok, ComponentState::Ok);
        assert!(report.ok);
        assert_eq!(report.ffmpeg, ComponentState::Ok);
        assert_eq!(report.model, ComponentState::Ok);
    }

    #[test]
    fn missing_ffmpeg_fails() {
        let report = evaluate(ComponentState::Missing, ComponentState::Ok);
        assert!(!report.ok);
        assert_eq!(report.ffmpeg, ComponentState::Missing);
        assert_eq!(report.model, ComponentState::Ok);
    }

    #[test]
    fn not_executable_ffmpeg_fails() {
        let report = evaluate(ComponentState::NotExecutable, ComponentState::Ok);
        assert!(!report.ok);
        assert_eq!(report.ffmpeg, ComponentState::NotExecutable);
    }

    #[test]
    fn missing_model_fails() {
        let report = evaluate(ComponentState::Ok, ComponentState::Missing);
        assert!(!report.ok);
        assert_eq!(report.model, ComponentState::Missing);
    }

    #[test]
    fn both_missing_fails() {
        let report = evaluate(ComponentState::Missing, ComponentState::Missing);
        assert!(!report.ok);
        assert_eq!(report.ffmpeg, ComponentState::Missing);
        assert_eq!(report.model, ComponentState::Missing);
    }

    #[test]
    fn probe_file_ok_when_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let rel = "resources/models/base.pt";
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"model bytes").unwrap();

        assert_eq!(probe_file(Some(dir.path()), rel), ComponentState::Ok);
    }

    #[test]
    fn probe_file_missing_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            probe_file(Some(dir.path()), "resources/models/base.pt"),
            ComponentState::Missing
        );
    }

    #[test]
    fn probe_file_missing_when_no_resource_dir() {
        assert_eq!(probe_file(None, "anything"), ComponentState::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn probe_executable_distinguishes_exec_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let rel = "resources/ffmpeg/ffmpeg";
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            probe_executable(Some(dir.path()), rel),
            ComponentState::NotExecutable
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(probe_executable(Some(dir.path()), rel), ComponentState::Ok);
    }
}
