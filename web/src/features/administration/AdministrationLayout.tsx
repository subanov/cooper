import { Link, Navigate, Outlet } from "@tanstack/react-router";
import { Settings } from "lucide-react";
import { EmptyState } from "@/components/EmptyState";
import { Page, PageHeader } from "@/components/Page";
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

/** Administration: a grouped list of pages beside the open page. */
export function AdministrationLayout() {
  const t = useText();
  const sections = useVisibleSections();
  return (
    <Page width="full" className="max-w-[90rem]">
      <PageHeader title={t("admin.title")} description={t("admin.intro")} />
      <div className="grid gap-6 lg:grid-cols-[14rem_minmax(0,1fr)]">
        <nav aria-label={t("admin.sections")} className="lg:sticky lg:top-0 lg:self-start">
          <div className="grid gap-4 sm:grid-cols-3 lg:grid-cols-1">
            {ADMIN_GROUPS.map((group) => {
              const items = sections.filter((s) => s.group === group.id);
              if (items.length === 0) return null;
              return (
                <div key={group.id}>
                  <p className="px-2 pb-1 text-[0.6875rem] font-semibold uppercase tracking-wider text-fg-muted">
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
        <div className="grid min-w-0 content-start gap-4">
          <Outlet />
        </div>
      </div>
    </Page>
  );
}

/** `/administration`: opens the first page this Analyst may use. */
export function AdministrationIndex() {
  const t = useText();
  const first = useVisibleSections()[0];
  if (first) return <Navigate to={first.to} replace />;
  return <EmptyState icon={<Settings aria-hidden="true" />} title={t("admin.nothing")} />;
}
