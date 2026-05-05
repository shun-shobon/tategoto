import { TriangleAlert } from "lucide-react";

type WarningBannerProps = {
  message: string;
};

export function WarningBanner({ message }: WarningBannerProps) {
  return (
    <section
      className="flex items-start gap-3 rounded-lg border border-amber-200 bg-amber-50 p-4 text-[13px] leading-relaxed text-amber-900"
      aria-label="最後の警告"
    >
      <TriangleAlert size={18} />
      <p>{message}</p>
    </section>
  );
}
