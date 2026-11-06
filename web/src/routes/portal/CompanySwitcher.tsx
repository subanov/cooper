import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { switchRequester } from "@/features/portal/actions";
import { portalSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

/**
 * The Company the Portal session acts for. One Portal Account can act for every active
 * Requester with its email address (ADR-0021); with more than one, the Requester picks
 * here, and every Portal screen reloads for that Company.
 */
export function CompanySwitcher({ session }: { session: Schemas["PortalSessionDto"] }) {
  const t = useText();
  const queryClient = useQueryClient();
  const [failed, setFailed] = useState(false);
  const current = session.requester;

  async function choose(requesterId: string) {
    if (requesterId === current.id) {
      return;
    }
    setFailed(false);
    const result = await switchRequester(requesterId);
    if (!result.ok) {
      setFailed(true);
      return;
    }
    queryClient.setQueryData(portalSessionQuery.queryKey, result.data);
    await queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] !== "session" });
  }

  return (
    <div className="flex items-center gap-2">
      {session.requesters.length > 1 ? (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="outline" size="sm">
              {t("portal.actingFor", { company: current.company_name })}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuLabel>{t("portal.switchCompany")}</DropdownMenuLabel>
            <DropdownMenuRadioGroup value={current.id} onValueChange={choose}>
              {session.requesters.map((r) => (
                <DropdownMenuRadioItem key={r.id} value={r.id}>
                  {r.company_name}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      ) : (
        <span className="text-sm text-muted-foreground">{current.company_name}</span>
      )}
      <span role="status" className="text-sm text-destructive">
        {failed ? t("portal.switchFailed") : ""}
      </span>
    </div>
  );
}
