import { Link, Outlet } from "@tanstack/react-router";
import { useText } from "@/i18n/useText";

const TABS = [
  { to: "/users/analysts", label: "users.analysts" },
  { to: "/users/requesters", label: "users.requesters" },
] as const;

/** Users: Analysts and Requesters, as tabs. */
export function UsersLayout() {
  const t = useText();
  return (
    <div className="grid gap-4">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("nav.users")}
      </h1>
      <nav aria-label={t("users.tabs")} className="flex gap-1 border-b border-border">
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
