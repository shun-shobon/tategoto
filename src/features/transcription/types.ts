export type TranscriptionStatus = "idle" | "recording" | "rotating_session" | "stopped_with_error";

export type InputDeviceMode = "system_default" | "fixed_device";

export type TranscriptionModel = "gpt-4o-transcribe" | "gpt-4o-mini-transcribe";

export type NoiseReductionType = "near_field" | "far_field";

export type InputDevice = {
  id: string;
  name: string;
  is_default: boolean;
};

export type TurnDetectionSettings = {
  threshold: number;
  prefix_padding_ms: number;
  silence_duration_ms: number;
};

export type TranscriptionSettings = {
  model: TranscriptionModel;
  language: string | null;
  prompt: string | null;
  noise_reduction: NoiseReductionType | null;
  turn_detection: TurnDetectionSettings;
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
  last_warning: string | null;
};

export const defaultTranscriptionSettings: TranscriptionSettings = {
  model: "gpt-4o-transcribe",
  language: null,
  prompt: null,
  noise_reduction: null,
  turn_detection: {
    threshold: 0.5,
    prefix_padding_ms: 300,
    silence_duration_ms: 700,
  },
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
  last_warning: null,
};
