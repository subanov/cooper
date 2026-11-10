import { Link, Outlet } from "@tanstack/react-router";
import { useText } from "@/i18n/useText";

const TABS = [
  { to: "/administration/settings", label: "admin.settings" },
  { to: "/administration/modules", label: "admin.modules" },
  { to: "/administration/jobs", label: "admin.jobs" },
  { to: "/administration/log", label: "admin.log" },
] as const;

/** Administration (FR-SYS-001 to FR-SYS-003): a heading, section tabs and the section. */
export function AdministrationLayout() {
  const t = useText();
  return (
    <div className="grid gap-4">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("admin.title")}
      </h1>
      <nav aria-label={t("admin.tabs")} className="flex flex-wrap gap-1 border-b border-border">
        {TABS.map((tab) => (
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
