import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useText } from "@/i18n/useText";

const TABS = [
  { to: "/contracts", label: "contracts.contracts" },
  { to: "/suppliers", label: "contracts.suppliers" },
] as const;

/** The Contracts heading with its Contracts and Suppliers tabs, and an optional action. */
export function ContractsTabs({ action }: { action?: ReactNode }) {
  const t = useText();
  return (
    <div className="grid gap-4">
      <div className="flex items-center gap-3">
        <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
          {t("nav.contracts")}
        </h1>
        {action ? <div className="ml-auto">{action}</div> : null}
      </div>
      <nav aria-label={t("contracts.tabs")} className="flex gap-1 border-b border-border">
        {TABS.map((tab) => (
          <Link
            key={tab.to}
            to={tab.to}
            activeOptions={{ exact: true }}
            className="-mb-px border-b-2 border-transparent px-3 py-2 text-sm hover:text-foreground"
            activeProps={{ className: "border-primary font-medium", "aria-current": "page" }}
          >
            {t(tab.label)}
          </Link>
        ))}
      </nav>
    </div>
  );
}
