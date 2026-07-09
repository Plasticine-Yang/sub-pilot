import { invoke } from "@tauri-apps/api/core";

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
