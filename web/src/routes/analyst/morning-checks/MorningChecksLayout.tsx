import { useQuery } from "@tanstack/react-query";
import { Link, Outlet } from "@tanstack/react-router";
import { can } from "@/features/navigation/navigation";
import { analystSessionQuery } from "@/features/session/queries";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";

/** Morning Checks: the day's results and (with `morning-checks.configure`) the Checks. */
export function MorningChecksLayout() {
  const t = useText();
  const session = useQuery(analystSessionQuery).data;
  const canConfigure = session ? can(session, "morning-checks.configure") : false;
  const tabs: { to: string; label: MessageId }[] = [
    { to: "/morning-checks/today", label: "checks.today" },
    ...(canConfigure ? [{ to: "/morning-checks/checks", label: "checks.checks" as const }] : []),
  ];
  return (
    <div className="grid gap-4">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("nav.morningChecks")}
      </h1>
      <nav aria-label={t("checks.tabs")} className="flex gap-1 border-b border-border">
        {tabs.map((tab) => (
          <Link
            key={tab.to}
            to={tab.to}
            className="-mb-px border-b-2 border-transparent px-3 py-2 text-sm hover:text-foreground"
            activeProps={{ className: "border-primary font-medium", "aria-current": "page" }}
          >
            {t(tab.label)}
          </Link>
        ))}
      </nav>
      <Outlet />
    </div>
  );
}
