import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { AppSnapshot, Settings } from "./types";

type SnapshotCommand =
  | "get_snapshot"
  | "start_transcription"
  | "stop_transcription"
  | "refresh_input_devices"
  | "open_today_markdown"
  | "open_output_directory";

type SnapshotEvent =
  | "transcription_state_changed"
  | "transcript_segment_written"
  | "transcription_error";

const snapshotEvents: SnapshotEvent[] = [
  "transcription_state_changed",
  "transcript_segment_written",
  "transcription_error",
];

export async function invokeSnapshot(command: SnapshotCommand): Promise<AppSnapshot> {
  return await invoke<AppSnapshot>(command);
}

export async function updateSettings(settings: Settings): Promise<AppSnapshot> {
  return await invoke<AppSnapshot>("update_settings", { settings });
}

export function listenToSnapshotEvents(onSnapshot: (snapshot: AppSnapshot) => void) {
  return snapshotEvents.map((eventName) =>
    listen<AppSnapshot>(eventName, (event) => {
      onSnapshot(event.payload);
    }),
  );
}
