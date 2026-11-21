import { TabNav } from "@/components/Tabs";
import { useText } from "@/i18n/useText";

/** Switches between Contracts and Suppliers. */
export function ContractsTabs() {
  const t = useText();
  return (
    <TabNav
      label={t("contracts.sections")}
      className="-mx-1 border-b border-line px-1"
      items={[
        { to: "/contracts", label: t("contracts.contracts") },
        { to: "/suppliers", label: t("contracts.suppliers") },
      ]}
    />
  );
}
