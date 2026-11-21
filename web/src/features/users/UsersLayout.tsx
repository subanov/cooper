import { Outlet } from "@tanstack/react-router";
import { SectionFrame } from "@/components/MasterDetail";
import type { TabLink } from "@/components/Tabs";
import { useCan } from "@/features/session/session";
import { useText } from "@/i18n/useText";

/** Analysts and Requesters, each shown to those who may manage or read them. */
export function UsersLayout() {
  const t = useText();
  const can = useCan();
  const tabs: TabLink[] = [];
  if (can("analysts.manage")) tabs.push({ to: "/users/analysts", label: t("users.analysts") });
  if (can("requesters.read")) tabs.push({ to: "/users/requesters", label: t("users.requesters") });
  return (
    <SectionFrame
      title={t("users.title")}
      description={t("users.intro")}
      tabsLabel={t("users.sections")}
      tabs={tabs}
    >
      <Outlet />
    </SectionFrame>
  );
}
