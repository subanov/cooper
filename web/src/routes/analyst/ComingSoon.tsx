import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";

/** Placeholder for a navigation entry whose screen is not built yet. */
export function ComingSoon({ section }: { section: MessageId }) {
  const t = useText();
  return (
    <div className="grid gap-2">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t(section)}
      </h1>
      <p className="text-muted-foreground">{t("comingSoon.body")}</p>
    </div>
  );
}
