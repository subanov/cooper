import { useQuery } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import { AppShell } from "./AppShell";

/** Start page of the Analyst console. Sign-in and navigation arrive with #81. */
export function AnalystHome() {
  const t = useText();
  const session = useQuery(analystSessionQuery);
  return (
    <AppShell title={t("analyst.title")}>
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle>{t("analyst.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          {session.isPending ? (
            <p role="status">{t("common.loading")}</p>
          ) : session.isError ? (
            <p role="alert">{t("common.error")}</p>
          ) : session.data ? (
            <p>{t("analyst.signedInAs", { name: session.data.analyst.full_name })}</p>
          ) : (
            <>
              <p>{t("analyst.notSignedIn")}</p>
              <p className="text-muted-foreground">{t("analyst.signInSoon")}</p>
            </>
          )}
        </CardContent>
      </Card>
    </AppShell>
  );
}
