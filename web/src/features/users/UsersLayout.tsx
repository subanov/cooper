import { Outlet } from "@tanstack/react-router";
import { Page, PageHeader } from "@/components/Page";
import { type NavTab, NavTabs } from "@/components/Tabs";
import { useCan } from "@/features/session/session";
import { useText } from "@/i18n/useText";

/** Analysts and Requesters, each shown to those who may manage or read them. */
export function UsersLayout() {
  const t = useText();
  const can = useCan();
  const tabs: NavTab[] = [];
  if (can("analysts.manage")) tabs.push({ to: "/users/analysts", label: t("users.analysts") });
  if (can("requesters.read")) tabs.push({ to: "/users/requesters", label: t("users.requesters") });
  return (
    <Page>
      <PageHeader title={t("users.title")} description={t("users.intro")} />
      {tabs.length > 1 ? <NavTabs label={t("users.sections")} items={tabs} /> : null}
      <Outlet />
    </Page>
  );
}
