import { useQuery } from "@tanstack/react-query";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { portalSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import { AppShell } from "./AppShell";

/** Start page of the Self-Service Portal. Sign-in and screens arrive with #82. */
export function PortalHome() {
  const t = useText();
  const session = useQuery(portalSessionQuery);
  return (
    <AppShell title={t("portal.title")}>
      <Card className="max-w-xl">
        <CardHeader>
          <CardTitle>{t("portal.title")}</CardTitle>
        </CardHeader>
        <CardContent>
          {session.isPending ? (
            <p role="status">{t("common.loading")}</p>
          ) : session.isError ? (
            <p role="alert">{t("common.error")}</p>
          ) : session.data ? (
            <p>
              {t("portal.signedInAs", {
                name: session.data.requester.full_name,
                company: session.data.requester.company_name,
              })}
            </p>
          ) : (
            <p>{t("portal.notSignedIn")}</p>
          )}
        </CardContent>
      </Card>
    </AppShell>
  );
}
