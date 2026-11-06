import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { portalSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";

/** Start page of the Self-Service Portal. */
export function PortalHome() {
  const t = useText();
  const { data: session } = useQuery(portalSessionQuery);
  if (!session) {
    return null;
  }
  return (
    <div className="grid max-w-2xl gap-3">
      <h1 tabIndex={-1} className="text-2xl font-semibold outline-none">
        {t("portalHome.welcome", { name: session.requester.full_name })}
      </h1>
      <p className="text-muted-foreground">
        {t("portalHome.intro", { company: session.requester.company_name })}
      </p>
      <p>
        <Link to="/portal/tickets" className="text-primary underline-offset-4 hover:underline">
          {t("portalHome.tickets")}
        </Link>
      </p>
    </div>
  );
}
