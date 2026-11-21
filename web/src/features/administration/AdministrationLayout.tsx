import { Link, Navigate, Outlet } from "@tanstack/react-router";
import { Settings } from "lucide-react";
import { EmptyState } from "@/components/Feedback";
import { useCan, useModuleOn } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { ADMIN_GROUPS, ADMIN_SECTIONS, type AdminSection } from "./sections";

function useVisibleSections(): AdminSection[] {
  const can = useCan();
  const moduleOn = useModuleOn();
  return ADMIN_SECTIONS.filter(
    (s) => s.capabilities.some(can) && (!s.module || moduleOn(s.module)),
  );
}

/**
 * Administration as a settings screen (as in most tools: Jakob's Law): grouped pages on the
 * left, the open page on the right. Only pages the Analyst may use are listed.
 */
export function AdministrationLayout() {
  const t = useText();
  const sections = useVisibleSections();
  return (
    <div className="flex h-full min-h-0 flex-col lg:flex-row">
      <aside className="shrink-0 border-b border-line bg-surface lg:w-60 lg:overflow-y-auto lg:border-b-0 lg:border-r">
        <div className="px-4 pb-2 pt-4">
          <h1 tabIndex={-1} data-page-heading className="text-lg font-semibold tracking-tight">
            {t("admin.title")}
          </h1>
          <p className="text-xs text-fg-muted">{t("admin.intro")}</p>
        </div>
        <nav
          aria-label={t("admin.sections")}
          className="max-h-56 overflow-y-auto px-2 pb-4 lg:max-h-none"
        >
          <div className="grid gap-3 sm:grid-cols-3 lg:grid-cols-1">
            {ADMIN_GROUPS.map((group) => {
              const items = sections.filter((s) => s.group === group.id);
              if (items.length === 0) return null;
              return (
                <div key={group.id}>
                  <p className="px-2 pb-1 pt-2 text-[0.6875rem] font-semibold uppercase tracking-wider text-fg-muted">
                    {t(group.label)}
                  </p>
                  <ul className="grid gap-0.5">
                    {items.map((s) => (
                      <li key={String(s.to)}>
                        <Link
                          to={s.to}
                          activeProps={{ "aria-current": "page" }}
                          className="flex h-8 items-center rounded-md px-2 text-[0.8125rem] font-medium text-fg-muted hover:bg-sunken hover:text-fg aria-[current=page]:bg-accent-soft aria-[current=page]:text-accent-text"
                        >
                          {t(s.label)}
                        </Link>
                      </li>
                    ))}
                  </ul>
                </div>
              );
            })}
          </div>
        </nav>
      </aside>
      <div className="min-h-0 min-w-0 flex-1 overflow-y-auto">
        <div className="mx-auto grid w-full max-w-6xl content-start gap-5 p-4 sm:p-6">
          <Outlet />
        </div>
      </div>
    </div>
  );
}

/** `/administration`: opens the first page this Analyst may use. */
export function AdministrationIndex() {
  const t = useText();
  const first = useVisibleSections()[0];
  if (first) return <Navigate to={first.to} replace />;
  return <EmptyState icon={<Settings aria-hidden="true" />} title={t("admin.nothing")} />;
}
