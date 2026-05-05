import { RefreshCcw } from "lucide-react";

import type { InputDevice, InputDeviceMode } from "../types";

type DeviceSectionProps = {
  devices: InputDevice[];
  mode: InputDeviceMode;
  pending: boolean;
  selectedDeviceId: string;
  selectedDeviceName: string;
  onDeviceChange: (deviceId: string) => void;
  onModeChange: (mode: InputDeviceMode) => void;
  onRefresh: () => void;
};

export function DeviceSection({
  devices,
  mode,
  pending,
  selectedDeviceId,
  selectedDeviceName,
  onDeviceChange,
  onModeChange,
  onRefresh,
}: DeviceSectionProps) {
  return (
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
          onClick={onRefresh}
          disabled={pending}
        >
          <RefreshCcw size={18} />
        </button>
      </div>

      <div className="grid grid-cols-2 overflow-hidden rounded-lg border border-slate-300">
        <button
          type="button"
          className={`min-h-9 bg-slate-50 font-bold text-slate-600 disabled:cursor-not-allowed disabled:opacity-55 ${
            mode === "system_default" ? "bg-blue-600 text-white" : ""
          }`}
          onClick={() => onModeChange("system_default")}
          disabled={pending}
        >
          Default
        </button>
        <button
          type="button"
          className={`min-h-9 border-l border-slate-300 bg-slate-50 font-bold text-slate-600 disabled:cursor-not-allowed disabled:opacity-55 ${
            mode === "fixed_device" ? "bg-blue-600 text-white" : ""
          }`}
          onClick={() => onModeChange("fixed_device")}
          disabled={pending || devices.length === 0}
        >
          Fixed
        </button>
      </div>

      <select
        className="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
        value={selectedDeviceId}
        onChange={(event) => onDeviceChange(event.currentTarget.value)}
        disabled={pending || devices.length === 0}
      >
        <option value="">デバイスを選択</option>
        {devices.map((device) => (
          <option key={device.id} value={device.id}>
            {device.is_default ? "Default: " : ""}
            {device.name}
          </option>
        ))}
      </select>
    </section>
  );
}
