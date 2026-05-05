import { Power } from "lucide-react";

import { DeviceSection } from "./features/transcription/components/DeviceSection";
import { ErrorBanner } from "./features/transcription/components/ErrorBanner";
import { OutputSection } from "./features/transcription/components/OutputSection";
import { RecordingControls } from "./features/transcription/components/RecordingControls";
import { StatusPill } from "./features/transcription/components/StatusPill";
import { WarningBanner } from "./features/transcription/components/WarningBanner";
import { useTranscriptionController } from "./features/transcription/useTranscriptionController";

export function App() {
  const transcription = useTranscriptionController();
  const { snapshot, pending, selectedDeviceId, selectedDeviceName } = transcription;

  return (
    <main className="flex min-h-screen min-w-[360px] flex-col gap-[18px] bg-slate-50 p-6 text-slate-800">
      <header className="flex items-center justify-between gap-4">
        <div>
          <p className="text-xs font-bold text-slate-500 uppercase">Tategoto</p>
          <h1 className="mt-0.5 text-[28px] leading-tight font-bold">文字起こし</h1>
        </div>
        <StatusPill status={snapshot.status} />
      </header>

      <RecordingControls
        canStart={transcription.canStart}
        canStop={transcription.canStop}
        onStart={transcription.start}
        onStop={transcription.stop}
      />

      <DeviceSection
        devices={snapshot.devices}
        mode={snapshot.settings.input_device_mode}
        pending={pending}
        selectedDeviceId={selectedDeviceId}
        selectedDeviceName={selectedDeviceName}
        onDeviceChange={transcription.selectDevice}
        onModeChange={transcription.selectMode}
        onRefresh={transcription.refreshDevices}
      />

      <OutputSection
        outputDirectory={snapshot.output_directory}
        pending={pending}
        todayJsonlPath={snapshot.today_jsonl_path}
        todayMarkdownPath={snapshot.today_markdown_path}
        onOpenMarkdown={transcription.openMarkdown}
        onOpenOutputDirectory={transcription.openOutputDirectory}
      />

      {snapshot.last_error ? <ErrorBanner message={snapshot.last_error} /> : null}
      {snapshot.last_warning ? <WarningBanner message={snapshot.last_warning} /> : null}

      <footer className="mt-auto flex items-center justify-center gap-2 text-xs leading-normal text-slate-500">
        <Power size={14} />
        <span>Quit はメニューバーから実行できます</span>
      </footer>
    </main>
  );
}
