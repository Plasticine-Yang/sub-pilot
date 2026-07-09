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
  Failed = "failed",
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
export function importVideo(path: string): Promise<Project> {
  return invoke<Project>("import_video", { path });
}

/** Starts transcription on the backend; progress arrives via events. */
export function startTranscription(projectId: string): Promise<void> {
  return invoke<void>("start_transcription", { projectId });
}

/** Reads the generated original.srt for display. */
export function readOriginalSrt(projectId: string): Promise<string> {
  return invoke<string>("read_original_srt", { projectId });
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
