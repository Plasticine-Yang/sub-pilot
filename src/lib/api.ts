import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export enum ComponentState {
  Ok = "ok",
  Missing = "missing",
  NotExecutable = "notExecutable",
}

export interface SelfCheckReport {
  ok: boolean;
  ffmpeg: ComponentState;
  model: ComponentState;
}

/** First-launch self-check of bundled ffmpeg + base model. */
export function selfCheck(): Promise<SelfCheckReport> {
  return invoke<SelfCheckReport>("self_check");
}

/** Project lifecycle state machine (mirrors the Rust `ProjectStatus`). */
export enum ProjectStatus {
  Imported = "imported",
  Transcribing = "transcribing",
  Transcribed = "transcribed",
  PromptReady = "prompt_ready",
  TranslationImported = "translation_imported",
  Validated = "validated",
  Exported = "exported",
  Failed = "failed",
}

/** Selectable Whisper model (mirrors the Rust `ModelId`, kebab-case). */
export enum ModelId {
  Tiny = "tiny",
  Base = "base",
  Small = "small",
  Medium = "medium",
  LargeV3 = "large-v3",
}

/** A model's presence on disk, for the picker. */
export interface ModelStatus {
  id: ModelId;
  name: string;
  bundled: boolean;
  downloaded: boolean;
}

export interface Project {
  id: string;
  videoPath: string;
  videoFileName: string;
  durationMs: number;
  model: string;
  status: ProjectStatus;
  error?: string;
}

export interface ProjectLocation {
  projectDir: string;
  originalSrt: string;
}

/** Imports a video file, creating a project; returns the new Project. */
export function importVideo(path: string, model?: ModelId): Promise<Project> {
  return invoke<Project>("import_video", { path, model });
}

/** A recent-projects list entry (mirrors the Rust `RecentEntry`). */
export interface RecentEntry {
  id: string;
  videoFileName: string;
  status: ProjectStatus;
  updatedAt: number;
}

/** Lists recent projects, reconciled against what still exists on disk. */
export function listRecent(): Promise<RecentEntry[]> {
  return invoke<RecentEntry[]>("list_recent");
}

/** A reopened project restored from disk (mirrors the Rust `OpenedProject`). */
export interface OpenedProject {
  project: Project;
  originalSrt?: string;
  translatedSrt?: string;
}

/** Reopens a recent project, restoring its persisted state. */
export function openProject(projectId: string): Promise<OpenedProject> {
  return invoke<OpenedProject>("open_project", { projectId });
}

/** Lists selectable models and whether each is present on disk. */
export function listModels(): Promise<ModelStatus[]> {
  return invoke<ModelStatus[]>("list_models");
}

/** Starts a model download on the backend; progress arrives via events. */
export function downloadModel(model: ModelId): Promise<void> {
  return invoke<void>("download_model", { model });
}

const EVENT_MODEL_DOWNLOAD = "model-download://event";

/** A model-download update pushed from the backend. */
export type ModelDownloadEvent =
  | { kind: "progress"; model: ModelId; downloaded: number; total: number | null }
  | { kind: "done"; model: ModelId }
  | { kind: "error"; model: ModelId; message: string };

/** Subscribes to model-download events; call the returned fn to unsubscribe. */
export function onModelDownloadEvent(
  handler: (event: ModelDownloadEvent) => void,
): Promise<UnlistenFn> {
  return listen<ModelDownloadEvent>(EVENT_MODEL_DOWNLOAD, (e) => handler(e.payload));
}

/** Starts transcription on the backend; progress arrives via events. */
export function startTranscription(projectId: string): Promise<void> {
  return invoke<void>("start_transcription", { projectId });
}

/** Reads the generated original.srt for display. */
export function readOriginalSrt(projectId: string): Promise<string> {
  return invoke<string>("read_original_srt", { projectId });
}

/** Generates the translation Prompt and advances the project to prompt_ready. */
export function generatePrompt(projectId: string): Promise<string> {
  return invoke<string>("generate_prompt", { projectId });
}

/** Outcome of importing a Translated Subtitle (mirrors Rust `ValidationResult`). */
export interface ValidationResult {
  ok: boolean;
  /** The validated SRT to display; present only when `ok`. */
  srt?: string;
  /** User-facing error message; present only when `!ok`. */
  message?: string;
  /** 1-based segment to locate the user to; present when the error is locatable. */
  segment?: number;
}

/** Imports a translated subtitle, validates it, and advances to validated. */
export function importTranslation(
  projectId: string,
  translatedPath: string,
): Promise<ValidationResult> {
  return invoke<ValidationResult>("import_translation", {
    projectId,
    translatedPath,
  });
}

/** Exports the validated subtitle as a sidecar .srt into outDir; returns path. */
export function exportSidecar(
  projectId: string,
  outDir: string,
): Promise<string> {
  return invoke<string>("export_sidecar", { projectId, outDir });
}

/** Muxes the subtitle into the video as a soft track in outDir; returns path. */
export function exportSoftSubtitle(
  projectId: string,
  outDir: string,
): Promise<string> {
  return invoke<string>("export_soft_subtitle", { projectId, outDir });
}

/** Starts burn-in export on the backend; progress arrives via export events. */
export function exportBurnIn(projectId: string, outDir: string): Promise<void> {
  return invoke<void>("export_burn_in", { projectId, outDir });
}

const EVENT_EXPORT = "export://event";

/** A burn-in export update pushed from the backend. */
export type ExportEvent =
  | { kind: "progress"; projectId: string; fraction: number }
  | { kind: "done"; projectId: string; path: string }
  | { kind: "error"; projectId: string; message: string };

/** Subscribes to burn-in export events; call the returned fn to unsubscribe. */
export function onExportEvent(
  handler: (event: ExportEvent) => void,
): Promise<UnlistenFn> {
  return listen<ExportEvent>(EVENT_EXPORT, (e) => handler(e.payload));
}

/** On-disk locations for "open subtitle" / "open directory". */
export function projectLocation(projectId: string): Promise<ProjectLocation> {
  return invoke<ProjectLocation>("project_location", { projectId });
}

const EVENT_TRANSCRIPTION = "transcription://event";

/** A transcription update pushed from the backend. */
export type TranscriptionEvent =
  | { kind: "status"; projectId: string; status: ProjectStatus }
  | { kind: "progress"; projectId: string; fraction: number; etaMs: number | null };

/** Subscribes to transcription events; call the returned fn to unsubscribe. */
export function onTranscriptionEvent(
  handler: (event: TranscriptionEvent) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptionEvent>(EVENT_TRANSCRIPTION, (e) => handler(e.payload));
}
