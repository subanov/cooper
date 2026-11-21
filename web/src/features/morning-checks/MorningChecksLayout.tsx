import { Outlet } from "@tanstack/react-router";
import { Page, PageHeader } from "@/components/Page";
import { NavTabs } from "@/components/Tabs";
import { useText } from "@/i18n/useText";

/** Morning Checks sections: today's Round and the Checks. */
export function MorningChecksLayout() {
  const t = useText();
  return (
    <Page width="wide">
      <PageHeader title={t("checks.title")} description={t("checks.intro")} />
      <NavTabs
        label={t("checks.sections")}
        items={[
          { to: "/morning-checks/today", label: t("checks.round") },
          { to: "/morning-checks/checks", label: t("checks.checks") },
        ]}
      />
      <Outlet />
    </Page>
  );
}
