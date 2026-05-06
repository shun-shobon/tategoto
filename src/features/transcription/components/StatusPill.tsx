import { Circle } from "lucide-react";

import type { TranscriptionStatus } from "../types";

const statusLabel: Record<TranscriptionStatus, string> = {
  idle: "待機中",
  recording: "録音中",
  stopped_with_error: "エラー停止",
};

const statusToneClass: Record<TranscriptionStatus, string> = {
  idle: "bg-slate-200 text-slate-600",
  recording: "bg-teal-100 text-teal-700",
  stopped_with_error: "bg-red-100 text-red-700",
};

type StatusPillProps = {
  status: TranscriptionStatus;
};

export function StatusPill({ status }: StatusPillProps) {
  return (
    <div
      className={`inline-flex min-w-33 items-center justify-center gap-2 rounded-full px-3 py-2 text-[13px] font-bold ${statusToneClass[status]}`}
    >
      <Circle size={12} fill="currentColor" />
      {statusLabel[status]}
    </div>
  );
}
