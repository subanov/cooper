import { Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { useText } from "@/i18n/useText";
import { AppShell } from "./AppShell";

/** Shown for unknown client-side routes. */
export function NotFound() {
  const t = useText();
  return (
    <AppShell title={t("notFound.title")}>
      <h1 className="mb-4 text-2xl font-semibold">{t("notFound.title")}</h1>
      <Button asChild variant="outline">
        <Link to="/">{t("notFound.back")}</Link>
      </Button>
    </AppShell>
  );
}
