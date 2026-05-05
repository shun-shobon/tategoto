import { useCallback, useEffect, useMemo, useState } from "react";

import { invokeSnapshot, listenToSnapshotEvents, updateSettings } from "./api";
import type { InputDeviceMode, Settings, TranscriptionSettings } from "./types";
import { defaultTranscriptionSettings, emptySnapshot } from "./types";

export function useTranscriptionController() {
  const [snapshot, setSnapshot] = useState(emptySnapshot);
  const [pending, setPending] = useState(false);

  const selectedDeviceId = snapshot.settings.input_device_id ?? "";
  const selectedDeviceName = useMemo(() => {
    if (snapshot.settings.input_device_mode === "system_default") {
      return snapshot.devices.find((device) => device.is_default)?.name ?? "システムデフォルト";
    }

    return snapshot.settings.input_device_name ?? "未選択";
  }, [snapshot.devices, snapshot.settings]);

  const refresh = useCallback(async () => {
    const nextSnapshot = await invokeSnapshot("get_snapshot");
    setSnapshot(nextSnapshot);
  }, []);

  useEffect(() => {
    void refresh();

    const unlisteners = listenToSnapshotEvents(setSnapshot);
    return () => {
      for (const unlisten of unlisteners) {
        void unlisten.then((dispose) => dispose());
      }
    };
  }, [refresh]);

  const runCommand = useCallback(async (command: Parameters<typeof invokeSnapshot>[0]) => {
    setPending(true);
    try {
      const nextSnapshot = await invokeSnapshot(command);
      setSnapshot(nextSnapshot);
    } finally {
      setPending(false);
    }
  }, []);

  const runSettingsCommand = useCallback(async (settings: Settings) => {
    setPending(true);
    try {
      const nextSnapshot = await updateSettings(settings);
      setSnapshot(nextSnapshot);
    } finally {
      setPending(false);
    }
  }, []);

  const runDeviceSettingsCommand = useCallback(
    async (mode: InputDeviceMode, deviceId: string | null) => {
      const device = deviceId ? snapshot.devices.find((item) => item.id === deviceId) : undefined;
      await runSettingsCommand({
        ...snapshot.settings,
        input_device_mode: mode,
        input_device_id: mode === "fixed_device" ? (device?.id ?? deviceId) : null,
        input_device_name: mode === "fixed_device" ? (device?.name ?? null) : null,
      });
    },
    [runSettingsCommand, snapshot.devices, snapshot.settings],
  );

  const updateTranscriptionSettings = useCallback(
    (transcription: TranscriptionSettings) => {
      void runSettingsCommand({
        ...snapshot.settings,
        transcription,
      });
    },
    [runSettingsCommand, snapshot.settings],
  );

  const resetTranscriptionSettings = useCallback(() => {
    void runSettingsCommand({
      ...snapshot.settings,
      transcription: {
        ...defaultTranscriptionSettings,
        turn_detection: { ...defaultTranscriptionSettings.turn_detection },
      },
    });
  }, [runSettingsCommand, snapshot.settings]);

  const selectMode = useCallback(
    (mode: InputDeviceMode) => {
      void runDeviceSettingsCommand(mode, selectedDeviceId || null);
    },
    [runDeviceSettingsCommand, selectedDeviceId],
  );

  const selectDevice = useCallback(
    (deviceId: string) => {
      if (!snapshot.devices.some((item) => item.id === deviceId)) {
        return;
      }
      void runDeviceSettingsCommand("fixed_device", deviceId);
    },
    [runDeviceSettingsCommand, snapshot.devices],
  );

  const canStart =
    !pending && (snapshot.status === "idle" || snapshot.status === "stopped_with_error");
  const canStop =
    !pending && (snapshot.status === "recording" || snapshot.status === "rotating_session");
  const settingsDisabled =
    pending || snapshot.status === "recording" || snapshot.status === "rotating_session";

  return {
    snapshot,
    pending,
    selectedDeviceId,
    selectedDeviceName,
    canStart,
    canStop,
    settingsDisabled,
    start: () => void runCommand("start_transcription"),
    stop: () => void runCommand("stop_transcription"),
    refreshDevices: () => void runCommand("refresh_input_devices"),
    openMarkdown: () => void runCommand("open_today_markdown"),
    openOutputDirectory: () => void runCommand("open_output_directory"),
    selectMode,
    selectDevice,
    updateTranscriptionSettings,
    resetTranscriptionSettings,
  };
}
