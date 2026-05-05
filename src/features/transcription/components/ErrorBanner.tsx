import { AlertCircle } from "lucide-react";

type ErrorBannerProps = {
  message: string;
};

export function ErrorBanner({ message }: ErrorBannerProps) {
  return (
    <section
      className="flex items-start gap-3 rounded-lg border border-rose-200 bg-rose-50 p-4 text-[13px] leading-relaxed text-red-800"
      aria-label="最後のエラー"
    >
      <AlertCircle size={18} />
      <p>{message}</p>
    </section>
  );
}
