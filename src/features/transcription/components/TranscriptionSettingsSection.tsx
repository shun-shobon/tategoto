import { RotateCcw } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import {
  noiseReductionOptions,
  transcriptionLanguageOptions,
  transcriptionModelOptions,
} from "../options";
import type { NoiseReductionType, TranscriptionModel, TranscriptionSettings } from "../types";

type TranscriptionSettingsSectionProps = {
  disabled: boolean;
  settings: TranscriptionSettings;
  onChange: (settings: TranscriptionSettings) => void;
  onReset: () => void;
};

export function TranscriptionSettingsSection({
  disabled,
  settings,
  onChange,
  onReset,
}: TranscriptionSettingsSectionProps) {
  const [draft, setDraft] = useState(settings);
  const draftRef = useRef(settings);
  const lastCommittedRef = useRef(settings);

  useEffect(() => {
    setDraft(settings);
    draftRef.current = settings;
    lastCommittedRef.current = settings;
  }, [settings]);

  const setDraftValue = (next: TranscriptionSettings) => {
    draftRef.current = next;
    setDraft(next);
  };

  const commit = (next: TranscriptionSettings) => {
    setDraftValue(next);
    if (JSON.stringify(next) === JSON.stringify(lastCommittedRef.current)) {
      return;
    }
    lastCommittedRef.current = next;
    onChange(next);
  };

  const updateDraft = (next: Partial<TranscriptionSettings>) => {
    setDraftValue({ ...draftRef.current, ...next });
  };

  const commitDraft = () => {
    commit(draftRef.current);
  };

  const updateTurnDetection = (next: Partial<TranscriptionSettings["turn_detection"]>) => {
    setDraftValue({
      ...draftRef.current,
      turn_detection: {
        ...draftRef.current.turn_detection,
        ...next,
      },
    });
  };

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

      <div className="grid gap-3">
        <label className="grid gap-1.5 text-[13px] font-bold text-slate-700">
          認識モデル
          <select
            className="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 font-normal text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
            value={draft.model}
            onChange={(event) => {
              commit({ ...draft, model: event.currentTarget.value as TranscriptionModel });
            }}
            disabled={disabled}
          >
            {transcriptionModelOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>

        <label className="grid gap-1.5 text-[13px] font-bold text-slate-700">
          音声の言語
          <select
            className="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 font-normal text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
            value={draft.language ?? ""}
            onChange={(event) => {
              commit({ ...draft, language: event.currentTarget.value || null });
            }}
            disabled={disabled}
          >
            {transcriptionLanguageOptions.map((option) => (
              <option key={option.value || "auto"} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>

        <label className="grid gap-1.5 text-[13px] font-bold text-slate-700">
          認識ヒント
          <textarea
            className="min-h-22 w-full resize-y rounded-lg border border-slate-300 bg-white px-3 py-2 font-normal text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
            value={draft.prompt ?? ""}
            onChange={(event) => {
              updateDraft({ prompt: event.currentTarget.value || null });
            }}
            onBlur={commitDraft}
            placeholder="固有名詞、専門用語、会話の文脈など"
            disabled={disabled}
          />
        </label>

        <label className="grid gap-1.5 text-[13px] font-bold text-slate-700">
          ノイズ低減
          <select
            className="min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 font-normal text-slate-800 disabled:cursor-not-allowed disabled:opacity-55"
            value={draft.noise_reduction ?? ""}
            onChange={(event) => {
              commit({
                ...draft,
                noise_reduction: (event.currentTarget.value || null) as NoiseReductionType | null,
              });
            }}
            disabled={disabled}
          >
            {noiseReductionOptions.map((option) => (
              <option key={option.value || "disabled"} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="grid gap-3 border-t border-slate-200 pt-4">
        <h3 className="text-[13px] leading-snug font-bold text-slate-700">発話区切り</h3>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <label className="grid gap-2 text-xs font-bold text-slate-600">
            <span className="flex items-center justify-between gap-2">
              反応しきい値
              <span className="font-mono text-[12px] font-normal text-slate-500">
                {draft.turn_detection.threshold.toFixed(2)}
              </span>
            </span>
            <input
              className="h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-200 accent-blue-600 disabled:cursor-not-allowed disabled:opacity-55"
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={draft.turn_detection.threshold}
              onChange={(event) => {
                updateTurnDetection({
                  threshold: Number(event.currentTarget.value),
                });
              }}
              onPointerUp={commitDraft}
              onKeyUp={commitDraft}
              onBlur={commitDraft}
              disabled={disabled}
            />
          </label>
          <label className="grid gap-2 text-xs font-bold text-slate-600">
            <span className="flex items-center justify-between gap-2">
              開始前の余白
              <span className="font-mono text-[12px] font-normal text-slate-500">
                {draft.turn_detection.prefix_padding_ms}ms
              </span>
            </span>
            <input
              className="h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-200 accent-blue-600 disabled:cursor-not-allowed disabled:opacity-55"
              type="range"
              min="0"
              max="1000"
              step="50"
              value={draft.turn_detection.prefix_padding_ms}
              onChange={(event) => {
                updateTurnDetection({
                  prefix_padding_ms: Number(event.currentTarget.value),
                });
              }}
              onPointerUp={commitDraft}
              onKeyUp={commitDraft}
              onBlur={commitDraft}
              disabled={disabled}
            />
          </label>
          <label className="grid gap-2 text-xs font-bold text-slate-600">
            <span className="flex items-center justify-between gap-2">
              無音で区切る時間
              <span className="font-mono text-[12px] font-normal text-slate-500">
                {draft.turn_detection.silence_duration_ms}ms
              </span>
            </span>
            <input
              className="h-2 w-full cursor-pointer appearance-none rounded-full bg-slate-200 accent-blue-600 disabled:cursor-not-allowed disabled:opacity-55"
              type="range"
              min="0"
              max="2000"
              step="50"
              value={draft.turn_detection.silence_duration_ms}
              onChange={(event) => {
                updateTurnDetection({
                  silence_duration_ms: Number(event.currentTarget.value),
                });
              }}
              onPointerUp={commitDraft}
              onKeyUp={commitDraft}
              onBlur={commitDraft}
              disabled={disabled}
            />
          </label>
        </div>
      </div>
    </section>
  );
}
