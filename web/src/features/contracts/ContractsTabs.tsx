import { NavTabs } from "@/components/Tabs";
import { useText } from "@/i18n/useText";

/** Switches between Contracts and Suppliers. */
export function ContractsTabs() {
  const t = useText();
  return (
    <NavTabs
      label={t("contracts.sections")}
      items={[
        { to: "/contracts", label: t("contracts.contracts") },
        { to: "/suppliers", label: t("contracts.suppliers") },
      ]}
    />
  );
}
