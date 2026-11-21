import { Outlet } from "@tanstack/react-router";
import { SectionFrame } from "@/components/MasterDetail";
import { useText } from "@/i18n/useText";

/** Morning Checks sections: today's Round and the Checks behind it. */
export function MorningChecksLayout() {
  const t = useText();
  return (
    <SectionFrame
      title={t("checks.title")}
      description={t("checks.intro")}
      tabsLabel={t("checks.sections")}
      tabs={[
        { to: "/morning-checks/today", label: t("checks.round") },
        { to: "/morning-checks/checks", label: t("checks.checks") },
      ]}
    >
      <Outlet />
    </SectionFrame>
  );
}
