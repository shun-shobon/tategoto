import { FolderOpen } from "lucide-react";

type OutputSectionProps = {
  outputDirectory: string;
  pending: boolean;
  todayJsonlPath: string;
  todayMarkdownPath: string;
  onOpenMarkdown: () => void;
  onOpenOutputDirectory: () => void;
};

export function OutputSection({
  outputDirectory,
  pending,
  todayJsonlPath,
  todayMarkdownPath,
  onOpenMarkdown,
  onOpenOutputDirectory,
}: OutputSectionProps) {
  return (
    <section
      className="flex flex-col gap-3.5 rounded-lg border border-slate-200 bg-white p-4"
      aria-label="出力"
    >
      <div className="flex items-center justify-between gap-4">
        <div>
          <h2 className="text-[15px] leading-snug font-bold">出力先</h2>
          <p className="text-xs leading-normal break-all text-slate-500">
            {outputDirectory || "未初期化"}
          </p>
          <p className="mt-2 text-xs leading-normal break-all text-slate-500">
            Markdown: {todayMarkdownPath || "未初期化"}
          </p>
          <p className="text-xs leading-normal break-all text-slate-500">
            JSONL: {todayJsonlPath || "未初期化"}
          </p>
        </div>
      </div>
      <div className="flex items-center gap-2.5">
        <button
          className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-slate-100 px-3 text-[13px] font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
          type="button"
          onClick={onOpenMarkdown}
          disabled={pending}
        >
          <FolderOpen size={17} />
          今日のMarkdown
        </button>
        <button
          className="inline-flex min-h-10.5 flex-1 items-center justify-center gap-2 rounded-lg bg-slate-100 px-3 text-[13px] font-bold text-slate-700 disabled:cursor-not-allowed disabled:opacity-55"
          type="button"
          onClick={onOpenOutputDirectory}
          disabled={pending}
        >
          <FolderOpen size={17} />
          ディレクトリ
        </button>
      </div>
    </section>
  );
}
