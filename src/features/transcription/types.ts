export type TranscriptionStatus = "idle" | "recording" | "stopped_with_error";

export type InputDeviceMode = "system_default" | "fixed_device";

export type InputDevice = {
  id: string;
  name: string;
  is_default: boolean;
};

export type TranscriptionSettings = {
  locale_identifier: string | null;
};

export type Settings = {
  input_device_mode: InputDeviceMode;
  input_device_id: string | null;
  input_device_name: string | null;
  transcription: TranscriptionSettings;
};

export type AppSnapshot = {
  status: TranscriptionStatus;
  settings: Settings;
  devices: InputDevice[];
  output_directory: string;
  today_markdown_path: string;
  today_jsonl_path: string;
  last_error: string | null;
};

export const defaultTranscriptionSettings: TranscriptionSettings = {
  locale_identifier: null,
};

export const emptySnapshot: AppSnapshot = {
  status: "idle",
  settings: {
    input_device_mode: "system_default",
    input_device_id: null,
    input_device_name: null,
    transcription: defaultTranscriptionSettings,
  },
  devices: [],
  output_directory: "",
  today_markdown_path: "",
  today_jsonl_path: "",
  last_error: null,
};
