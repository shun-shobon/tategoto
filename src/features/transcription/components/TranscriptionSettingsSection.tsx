import { RotateCcw } from "lucide-react";

import type { TranscriptionSettings } from "../types";

type TranscriptionSettingsSectionProps = {
  disabled: boolean;
  settings: TranscriptionSettings;
  onChange: (settings: TranscriptionSettings) => void;
  onReset: () => void;
};

const localeOptions: Array<{ value: string; label: string }> = [
  { value: "", label: "システム設定" },
  { value: "ja-JP", label: "日本語 (ja-JP)" },
  { value: "en-US", label: "English (en-US)" },
];

export function TranscriptionSettingsSection({
  disabled,
  settings,
  onChange,
  onReset,
}: TranscriptionSettingsSectionProps) {
  return (
    <section
      className="flex flex-col gap-4 rounded-lg border border-slate-200 bg-white p-4"
      aria-label="文字起こし設定"
    >
      <div className="flex items-center justify-between gap-4">
        <h2 className="text-[15px] leading-snug font-bold">文字起こし設定</h2>
        <button
          className="inline-flex min-h-9 items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-3 text-[13px] font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
          type="button"
          onClick={onReset}
          disabled={disabled}
        >
          <RotateCcw size={16} />
          リセット
        </button>
      </div>

      <label className="grid gap-1.5 text-[13px] font-bold text-slate-700">
        認識ロケール
        <select
          className="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 font-normal text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
          value={settings.locale_identifier ?? ""}
          onChange={(event) => {
            onChange({ locale_identifier: event.currentTarget.value || null });
          }}
          disabled={disabled}
        >
          {localeOptions.map((option) => (
            <option key={option.value || "auto"} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
    </section>
  );
}
