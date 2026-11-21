import { useState } from "react";
import { Button } from "@/components/Button";
import { useToast } from "@/components/Toast";
import { useCan, useSession } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { type Ticket, ticketActions } from "./api";
import { AssignDialog, PriorityDialog, StatusDialog } from "./dialogs";

type Dialog = "assign" | "status" | "priority" | null;

/**
 * Applies one action to the selected tickets (FR-TKT-042). Each ticket is changed on its
 * own with the usual checks; the outcome per ticket is reported.
 */
export function BulkBar({
  ids,
  tickets,
  onClear,
}: {
  ids: string[];
  tickets: Ticket[];
  onClear: () => void;
}) {
  const t = useText();
  const can = useCan();
  const session = useSession();
  const toast = useToast();
  const [dialog, setDialog] = useState<Dialog>(null);
  const bulk = useAction(
    (body: Omit<Schemas["BulkRequest"], "ticket_ids">) =>
      ticketActions.bulk({ ...body, ticket_ids: ids }),
    {
      invalidate: [["tickets"]],
      onSuccess: (results) => {
        const failed = results.filter((r) => !r.ok);
        const ok = results.length - failed.length;
        const byId = new Map(tickets.map((tk) => [tk.id, tk.number]));
        toast({
          tone: failed.length ? "danger" : "success",
          message: failed.length
            ? `${t("bulk.partial", { ok, failed: failed.length })} ${failed
                .map((f) => `${byId.get(f.ticket_id) ?? ""}: ${f.error ?? ""}`)
                .join("; ")}`
            : t("bulk.done", { ok }),
        });
        setDialog(null);
        if (!failed.length) onClear();
      },
    },
  );
  const canAssign = can("tickets.assign") || can("tickets.update");
  return (
    <section
      aria-label={t("bulk.selected", { count: ids.length })}
      className="flex flex-wrap items-center gap-2 border-b border-line bg-accent-soft px-4 py-2"
    >
      <p className="mr-auto text-[0.8125rem] font-semibold">
        {t("bulk.selected", { count: ids.length })}
      </p>
      {canAssign ? (
        <>
          <Button
            size="sm"
            loading={bulk.isPending && bulk.variables?.analyst_id === session.analyst.id}
            onClick={() => bulk.mutate({ action: "assign", analyst_id: session.analyst.id })}
          >
            {t("bulk.assignToMe")}
          </Button>
          <Button size="sm" onClick={() => setDialog("assign")}>
            {t("bulk.assign")}
          </Button>
        </>
      ) : null}
      {can("tickets.update") ? (
        <>
          <Button size="sm" onClick={() => setDialog("status")}>
            {t("bulk.status")}
          </Button>
          <Button size="sm" onClick={() => setDialog("priority")}>
            {t("bulk.priority")}
          </Button>
        </>
      ) : null}
      <Button size="sm" variant="ghost" onClick={onClear}>
        {t("bulk.clear")}
      </Button>
      <AssignDialog
        open={dialog === "assign"}
        onOpenChange={(open) => setDialog(open ? "assign" : null)}
        title={t("bulk.apply", { count: ids.length })}
        initial={{ team_id: null, analyst_id: null }}
        busy={bulk.isPending}
        error={bulk.error}
        onSubmit={(a) => bulk.mutate({ action: "assign", ...a })}
      />
      <StatusDialog
        open={dialog === "status"}
        onOpenChange={(open) => setDialog(open ? "status" : null)}
        title={t("bulk.apply", { count: ids.length })}
        busy={bulk.isPending}
        error={bulk.error}
        onSubmit={(change) => bulk.mutate({ action: "status", ...change })}
      />
      <PriorityDialog
        open={dialog === "priority"}
        onOpenChange={(open) => setDialog(open ? "priority" : null)}
        title={t("bulk.apply", { count: ids.length })}
        busy={bulk.isPending}
        error={bulk.error}
        onSubmit={(priority_id) => bulk.mutate({ action: "priority", priority_id })}
      />
    </section>
  );
}
