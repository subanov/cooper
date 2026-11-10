import { useQuery } from "@tanstack/react-query";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { statusOverviewQuery } from "@/features/service-status/api";
import { useText } from "@/i18n/useText";

/** The current impact level of every service. */
export function OverviewPage() {
  const t = useText();
  const overview = useQuery(statusOverviewQuery);
  if (overview.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (overview.isError) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  if (overview.data.length === 0) {
    return <p className="text-muted-foreground">{t("status.noServices")}</p>;
  }
  return (
    <table className="w-full text-left text-sm">
      <caption className="sr-only">{t("status.overview")}</caption>
      <thead>
        <tr className="border-b border-border">
          <th scope="col" className="py-2 pr-3">
            {t("status.service")}
          </th>
          <th scope="col" className="py-2">
            {t("status.impact")}
          </th>
        </tr>
      </thead>
      <tbody>
        {overview.data.map((row) => (
          <tr key={row.service.id} className="border-b border-border">
            <td className="py-2 pr-3">
              <span className="font-medium">{row.service.name}</span>
              {row.service.description ? (
                <span className="block text-xs text-muted-foreground">
                  {row.service.description}
                </span>
              ) : null}
            </td>
            <td className="py-2">
              <Badge
                tone={
                  row.status.counts_as_downtime
                    ? "breached"
                    : row.status.is_default
                      ? "ok"
                      : "warning"
                }
              >
                {row.status.name}
              </Badge>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
