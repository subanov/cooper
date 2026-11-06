import { useQuery } from "@tanstack/react-query";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";

/** Start page of the Analyst console. */
export function Home() {
  const t = useText();
  const { data: session } = useQuery(analystSessionQuery);
  if (!session) {
    return null;
  }
  return (
    <div className="grid max-w-2xl gap-3">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("home.welcome", { name: session.analyst.full_name })}
      </h1>
      <p className="text-muted-foreground">{t("home.intro", { shortcut: "Ctrl K" })}</p>
      <p>
        {session.all_companies
          ? t("home.allCompanies")
          : t("home.companies", { count: session.company_ids.length })}
      </p>
    </div>
  );
}
