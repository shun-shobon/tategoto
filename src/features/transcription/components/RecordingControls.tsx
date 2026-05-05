import { Mic, MicOff } from "lucide-react";

type RecordingControlsProps = {
  canStart: boolean;
  canStop: boolean;
  onStart: () => void;
  onStop: () => void;
};

export function RecordingControls({ canStart, canStop, onStart, onStop }: RecordingControlsProps) {
  return (
    <section className="flex items-center gap-3" aria-label="録音操作">
      <button
        className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-teal-700 px-4 font-bold text-white disabled:cursor-not-allowed disabled:opacity-55"
        type="button"
        onClick={onStart}
        disabled={!canStart}
      >
        <Mic size={18} />
        Start
      </button>
      <button
        className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-slate-200 px-4 font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
        type="button"
        onClick={onStop}
        disabled={!canStop}
      >
        <MicOff size={18} />
        Stop
      </button>
    </section>
  );
}
