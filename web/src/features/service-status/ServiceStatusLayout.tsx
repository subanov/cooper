import { Outlet } from "@tanstack/react-router";
import { SectionFrame } from "@/components/MasterDetail";
import { useText } from "@/i18n/useText";

/** Service Status sections: Overview, Incidents and Services. */
export function ServiceStatusLayout() {
  const t = useText();
  return (
    <SectionFrame
      title={t("status.title")}
      description={t("status.intro")}
      tabsLabel={t("status.sections")}
      tabs={[
        { to: "/service-status/overview", label: t("status.overview") },
        { to: "/service-status/incidents", label: t("status.incidents") },
        { to: "/service-status/services", label: t("status.services") },
      ]}
    >
      <Outlet />
    </SectionFrame>
  );
}
