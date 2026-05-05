import { useCallback, useEffect, useMemo, useState } from "react";

import { invokeSnapshot, listenToSnapshotEvents, updateSettings } from "./api";
import type { InputDeviceMode } from "./types";
import { emptySnapshot } from "./types";

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

  const runSettingsCommand = useCallback(
    async (mode: InputDeviceMode, deviceId: string | null) => {
      const device = deviceId ? snapshot.devices.find((item) => item.id === deviceId) : undefined;
      setPending(true);
      try {
        const nextSnapshot = await updateSettings({
          input_device_mode: mode,
          input_device_id: mode === "fixed_device" ? (device?.id ?? deviceId) : null,
          input_device_name: mode === "fixed_device" ? (device?.name ?? null) : null,
        });
        setSnapshot(nextSnapshot);
      } finally {
        setPending(false);
      }
    },
    [snapshot.devices],
  );

  const selectMode = useCallback(
    (mode: InputDeviceMode) => {
      void runSettingsCommand(mode, selectedDeviceId || null);
    },
    [runSettingsCommand, selectedDeviceId],
  );

  const selectDevice = useCallback(
    (deviceId: string) => {
      if (!snapshot.devices.some((item) => item.id === deviceId)) {
        return;
      }
      void runSettingsCommand("fixed_device", deviceId);
    },
    [runSettingsCommand, snapshot.devices],
  );

  const canStart =
    !pending && (snapshot.status === "idle" || snapshot.status === "stopped_with_error");
  const canStop =
    !pending && (snapshot.status === "recording" || snapshot.status === "rotating_session");

  return {
    snapshot,
    pending,
    selectedDeviceId,
    selectedDeviceName,
    canStart,
    canStop,
    start: () => void runCommand("start_transcription"),
    stop: () => void runCommand("stop_transcription"),
    refreshDevices: () => void runCommand("refresh_input_devices"),
    openMarkdown: () => void runCommand("open_today_markdown"),
    openOutputDirectory: () => void runCommand("open_output_directory"),
    selectMode,
    selectDevice,
  };
}
