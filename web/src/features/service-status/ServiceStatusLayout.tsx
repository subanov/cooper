import { Outlet } from "@tanstack/react-router";
import { Page, PageHeader } from "@/components/Page";
import { NavTabs } from "@/components/Tabs";
import { useText } from "@/i18n/useText";

/** Service Status sections: Overview, Incidents and Services. */
export function ServiceStatusLayout() {
  const t = useText();
  return (
    <Page>
      <PageHeader title={t("status.title")} description={t("status.intro")} />
      <NavTabs
        label={t("status.sections")}
        items={[
          { to: "/service-status/overview", label: t("status.overview") },
          { to: "/service-status/incidents", label: t("status.incidents") },
          { to: "/service-status/services", label: t("status.services") },
        ]}
      />
      <Outlet />
    </Page>
  );
}
