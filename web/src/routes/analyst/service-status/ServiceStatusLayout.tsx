import { Link, Outlet } from "@tanstack/react-router";
import { useText } from "@/i18n/useText";

const TABS = [
  { to: "/service-status/overview", label: "status.overview" },
  { to: "/service-status/incidents", label: "status.incidents" },
  { to: "/service-status/services", label: "status.services" },
] as const;

/** Service Status: current state, Incidents and the services themselves. */
export function ServiceStatusLayout() {
  const t = useText();
  return (
    <div className="grid gap-4">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("nav.serviceStatus")}
      </h1>
      <nav aria-label={t("status.tabs")} className="flex gap-1 border-b border-border">
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
