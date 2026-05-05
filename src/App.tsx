import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, Circle, FolderOpen, Mic, MicOff, Power, RefreshCcw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

type TranscriptionStatus = "idle" | "recording" | "rotating_session" | "stopped_with_error";

type InputDeviceMode = "system_default" | "fixed_device";

type InputDevice = {
  id: string;
  name: string;
  is_default: boolean;
};

type Settings = {
  input_device_mode: InputDeviceMode;
  input_device_id: string | null;
  input_device_name: string | null;
};

type AppSnapshot = {
  status: TranscriptionStatus;
  settings: Settings;
  devices: InputDevice[];
  output_directory: string;
  today_markdown_path: string;
  today_jsonl_path: string;
  last_error: string | null;
};

const statusLabel: Record<TranscriptionStatus, string> = {
  idle: "待機中",
  recording: "録音中",
  rotating_session: "セッション更新中",
  stopped_with_error: "エラー停止",
};

const statusToneClass: Record<TranscriptionStatus, string> = {
  idle: "bg-slate-200 text-slate-600",
  recording: "bg-teal-100 text-teal-700",
  rotating_session: "bg-orange-100 text-orange-800",
  stopped_with_error: "bg-red-100 text-red-700",
};

const emptySnapshot: AppSnapshot = {
  status: "idle",
  settings: {
    input_device_mode: "system_default",
    input_device_id: null,
    input_device_name: null,
  },
  devices: [],
  output_directory: "",
  today_markdown_path: "",
  today_jsonl_path: "",
  last_error: null,
};

export function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(emptySnapshot);
  const [pending, setPending] = useState(false);

  const selectedDeviceId = snapshot.settings.input_device_id ?? "";
  const selectedDeviceName = useMemo(() => {
    if (snapshot.settings.input_device_mode === "system_default") {
      return snapshot.devices.find((device) => device.is_default)?.name ?? "システムデフォルト";
    }

    return snapshot.settings.input_device_name ?? "未選択";
  }, [snapshot.devices, snapshot.settings]);

  const refresh = useCallback(async () => {
    const nextSnapshot = await invoke<AppSnapshot>("get_snapshot");
    setSnapshot(nextSnapshot);
  }, []);

  useEffect(() => {
    void refresh();

    const unlistenState = listen<AppSnapshot>("transcription_state_changed", (event) => {
      setSnapshot(event.payload);
    });
    const unlistenWritten = listen<AppSnapshot>("transcript_segment_written", (event) => {
      setSnapshot(event.payload);
    });
    const unlistenError = listen<AppSnapshot>("transcription_error", (event) => {
      setSnapshot(event.payload);
    });

    return () => {
      void unlistenState.then((unlisten) => unlisten());
      void unlistenWritten.then((unlisten) => unlisten());
      void unlistenError.then((unlisten) => unlisten());
    };
  }, [refresh]);

  const runCommand = useCallback(async (command: string, args?: Record<string, unknown>) => {
    setPending(true);
    try {
      const nextSnapshot = await invoke<AppSnapshot>(command, args);
      setSnapshot(nextSnapshot);
    } finally {
      setPending(false);
    }
  }, []);

  const handleStart = useCallback(() => {
    void runCommand("start_transcription");
  }, [runCommand]);

  const handleStop = useCallback(() => {
    void runCommand("stop_transcription");
  }, [runCommand]);

  const handleRefreshDevices = useCallback(() => {
    void runCommand("refresh_input_devices");
  }, [runCommand]);

  const handleOpenMarkdown = useCallback(() => {
    void runCommand("open_today_markdown");
  }, [runCommand]);

  const handleOpenOutputDirectory = useCallback(() => {
    void runCommand("open_output_directory");
  }, [runCommand]);

  const handleModeChange = useCallback(
    (mode: InputDeviceMode) => {
      const device = snapshot.devices.find((item) => item.id === selectedDeviceId);
      void runCommand("update_settings", {
        settings: {
          input_device_mode: mode,
          input_device_id: mode === "fixed_device" ? (device?.id ?? selectedDeviceId) : null,
          input_device_name: mode === "fixed_device" ? (device?.name ?? null) : null,
        },
      });
    },
    [runCommand, selectedDeviceId, snapshot.devices],
  );

  const handleDeviceChange = useCallback(
    (deviceId: string) => {
      const device = snapshot.devices.find((item) => item.id === deviceId);
      if (!device) {
        return;
      }

      void runCommand("update_settings", {
        settings: {
          input_device_mode: "fixed_device",
          input_device_id: device.id,
          input_device_name: device.name,
        },
      });
    },
    [runCommand, snapshot.devices],
  );

  const canStart =
    !pending && (snapshot.status === "idle" || snapshot.status === "stopped_with_error");
  const canStop =
    !pending && (snapshot.status === "recording" || snapshot.status === "rotating_session");

  return (
    <main className="flex min-h-screen min-w-[360px] flex-col gap-[18px] bg-slate-50 p-6 text-slate-800">
      <header className="flex items-center justify-between gap-4">
        <div>
          <p className="text-xs font-bold text-slate-500 uppercase">Tategoto</p>
          <h1 className="mt-0.5 text-[28px] leading-tight font-bold">文字起こし</h1>
        </div>
        <div
          className={`inline-flex min-w-33 items-center justify-center gap-2 rounded-full px-3 py-2 text-[13px] font-bold ${statusToneClass[snapshot.status]}`}
        >
          <Circle size={12} fill="currentColor" />
          {statusLabel[snapshot.status]}
        </div>
      </header>

      <section className="flex items-center gap-3" aria-label="録音操作">
        <button
          className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-teal-700 px-4 font-bold text-white disabled:cursor-not-allowed disabled:opacity-55"
          type="button"
          onClick={handleStart}
          disabled={!canStart}
        >
          <Mic size={18} />
          Start
        </button>
        <button
          className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-slate-200 px-4 font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
          type="button"
          onClick={handleStop}
          disabled={!canStop}
        >
          <MicOff size={18} />
          Stop
        </button>
      </section>

      <section
        className="flex flex-col gap-3.5 rounded-lg border border-slate-200 bg-white p-4"
        aria-label="入力デバイス"
      >
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="text-[15px] leading-snug font-bold">入力デバイス</h2>
            <p className="text-xs leading-normal text-slate-500">{selectedDeviceName}</p>
          </div>
          <button
            className="inline-grid size-9 place-items-center rounded-lg border border-slate-300 bg-white text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
            type="button"
            onClick={handleRefreshDevices}
            disabled={pending}
          >
            <RefreshCcw size={18} />
          </button>
        </div>

        <div className="grid grid-cols-2 overflow-hidden rounded-lg border border-slate-300">
          <button
            type="button"
            className={`min-h-9 bg-slate-50 font-bold text-slate-600 disabled:cursor-not-allowed disabled:opacity-55 ${
              snapshot.settings.input_device_mode === "system_default"
                ? "bg-blue-600 text-white"
                : ""
            }`}
            onClick={() => handleModeChange("system_default")}
            disabled={pending}
          >
            Default
          </button>
          <button
            type="button"
            className={`min-h-9 border-l border-slate-300 bg-slate-50 font-bold text-slate-600 disabled:cursor-not-allowed disabled:opacity-55 ${
              snapshot.settings.input_device_mode === "fixed_device" ? "bg-blue-600 text-white" : ""
            }`}
            onClick={() => handleModeChange("fixed_device")}
            disabled={pending || snapshot.devices.length === 0}
          >
            Fixed
          </button>
        </div>

        <select
          className="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
          value={selectedDeviceId}
          onChange={(event) => handleDeviceChange(event.currentTarget.value)}
          disabled={pending || snapshot.devices.length === 0}
        >
          <option value="">デバイスを選択</option>
          {snapshot.devices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.is_default ? "Default: " : ""}
              {device.name}
            </option>
          ))}
        </select>
      </section>

      <section
        className="flex flex-col gap-3.5 rounded-lg border border-slate-200 bg-white p-4"
        aria-label="出力"
      >
        <div className="flex items-center justify-between gap-4">
          <div>
            <h2 className="text-[15px] leading-snug font-bold">出力先</h2>
            <p className="text-xs leading-normal break-all text-slate-500">
              {snapshot.output_directory || "未初期化"}
            </p>
            <p className="mt-2 text-xs leading-normal break-all text-slate-500">
              Markdown: {snapshot.today_markdown_path || "未初期化"}
            </p>
            <p className="text-xs leading-normal break-all text-slate-500">
              JSONL: {snapshot.today_jsonl_path || "未初期化"}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2.5">
          <button
            className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-slate-100 px-3 text-[13px] font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
            type="button"
            onClick={handleOpenMarkdown}
            disabled={pending}
          >
            <FolderOpen size={17} />
            今日のMarkdown
          </button>
          <button
            className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-slate-100 px-3 text-[13px] font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
            type="button"
            onClick={handleOpenOutputDirectory}
            disabled={pending}
          >
            <FolderOpen size={17} />
            ディレクトリ
          </button>
        </div>
      </section>

      {snapshot.last_error ? (
        <section
          className="flex items-start gap-3 rounded-lg border border-rose-200 bg-rose-50 p-4 text-[13px] leading-relaxed text-red-800"
          aria-label="最後のエラー"
        >
          <AlertCircle size={18} />
          <p>{snapshot.last_error}</p>
        </section>
      ) : null}

      <footer className="mt-auto flex items-center justify-center gap-2 text-xs leading-normal text-slate-500">
        <Power size={14} />
        <span>Quit はメニューバーから実行できます</span>
      </footer>
    </main>
  );
}
