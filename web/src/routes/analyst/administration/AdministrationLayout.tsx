import { Link, Outlet } from "@tanstack/react-router";
import type { MessageId } from "@/i18n/messages";
import { useText } from "@/i18n/useText";

type Section = {
  heading: MessageId;
  links: readonly { to: string; label: MessageId }[];
};

const SECTIONS: readonly Section[] = [
  {
    heading: "admin.group.general",
    links: [
      { to: "/administration/settings", label: "admin.settings" },
      { to: "/administration/modules", label: "admin.modules" },
      { to: "/administration/jobs", label: "admin.jobs" },
      { to: "/administration/log", label: "admin.log" },
    ],
  },
  {
    heading: "admin.group.tickets",
    links: [
      { to: "/administration/ticket-statuses", label: "admin.statuses" },
      { to: "/administration/ticket-priorities", label: "admin.priorities" },
      { to: "/administration/ticket-types", label: "admin.types" },
      { to: "/administration/ticket-origins", label: "admin.origins" },
      { to: "/administration/ticket-categories", label: "admin.categories" },
      { to: "/administration/resolution-codes", label: "admin.codes" },
    ],
  },
  {
    heading: "admin.group.organisation",
    links: [
      { to: "/administration/companies", label: "admin.companies" },
      { to: "/administration/teams", label: "admin.teams" },
      { to: "/administration/departments", label: "admin.departments" },
      { to: "/administration/cost-centres", label: "admin.costCentres" },
      { to: "/administration/roles", label: "admin.roles" },
    ],
  },
  {
    heading: "admin.group.assets",
    links: [
      { to: "/administration/asset-types", label: "admin.assetTypes" },
      { to: "/administration/asset-statuses", label: "admin.assetStatuses" },
      { to: "/administration/asset-locations", label: "admin.locations" },
    ],
  },
  {
    heading: "admin.group.contracts",
    links: [
      { to: "/administration/supplier-types", label: "admin.supplierTypes" },
      { to: "/administration/supplier-statuses", label: "admin.supplierStatuses" },
      { to: "/administration/contract-statuses", label: "admin.contractStatuses" },
      { to: "/administration/payment-schedules", label: "admin.paymentSchedules" },
    ],
  },
  {
    heading: "admin.group.calendar",
    links: [{ to: "/administration/calendar-categories", label: "admin.calendarCategories" }],
  },
];

/** Administration (FR-SYS-001 to FR-SYS-003): a sectioned menu beside the chosen page. */
export function AdministrationLayout() {
  const t = useText();
  return (
    <div className="grid gap-4">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("admin.title")}
      </h1>
      <div className="grid gap-6 md:grid-cols-[14rem_1fr]">
        <nav aria-label={t("admin.tabs")} className="grid content-start gap-4">
          {SECTIONS.map((section) => (
            <div key={section.heading} className="grid gap-1">
              <h2 className="px-3 text-xs font-semibold uppercase text-muted-foreground">
                {t(section.heading)}
              </h2>
              <ul className="grid gap-0.5">
                {section.links.map((link) => (
                  <li key={link.to}>
                    <Link
                      to={link.to}
                      className="block rounded-md px-3 py-1.5 text-sm hover:bg-muted"
                      activeProps={{
                        className: "bg-muted font-medium",
                        "aria-current": "page",
                      }}
                    >
                      {t(link.label)}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>
        <div className="min-w-0">
          <Outlet />
        </div>
      </div>
    </div>
  );
}
