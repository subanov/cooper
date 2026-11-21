import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Combobox, type PickerOption } from "@/components/Combobox";
import type { FieldShellProps } from "@/components/Field";
import { api } from "@/lib/api";
import { call } from "@/lib/request";
import { companiesQuery, namesById } from "./queries";

type PickerProps = Omit<FieldShellProps, "id"> & {
  value: PickerOption | null;
  onChange: (option: PickerOption | null) => void;
  disabled?: boolean | undefined;
  placeholder?: string | undefined;
};

/** Finds a Requester by name or email as you type, optionally within one Company. */
export function RequesterPicker({
  companyId,
  ...props
}: PickerProps & { companyId?: string | undefined }) {
  const [q, setQ] = useState("");
  const companies = useQuery(companiesQuery);
  const companyNames = namesById(companies.data);
  const found = useQuery({
    queryKey: ["reference", "requester-search", q, companyId ?? null],
    queryFn: () =>
      call(
        api.GET("/api/v1/requesters", {
          params: { query: { limit: 20, q, ...(companyId ? { company_id: companyId } : {}) } },
        }),
      ),
    enabled: q.length > 0,
    staleTime: 30_000,
  });
  const options = (found.data?.items ?? []).map(
    (r): PickerOption => ({
      id: r.id,
      label: r.full_name,
      description: [r.email, companyId ? undefined : companyNames.get(r.company_id)]
        .filter(Boolean)
        .join(" · "),
    }),
  );
  return (
    <Combobox
      {...props}
      options={q ? options : []}
      loading={found.isFetching}
      onQueryChange={setQ}
    />
  );
}

type LinkPickerProps = PickerProps & {
  companyId?: string | undefined;
  /** Records already linked, left out of the list. */
  exclude?: ReadonlySet<string> | undefined;
  /** Before anything is typed, offer the Analyst's own open records (H6, recognition). */
  suggestMine?: boolean | undefined;
};

/**
 * Finds a ticket by number or subject as you type, optionally within one Company. With
 * `suggestMine`, an empty search lists the open tickets assigned to the signed-in Analyst.
 */
export function TicketPicker({
  companyId,
  exclude,
  suggestMine = false,
  ...props
}: LinkPickerProps) {
  const [q, setQ] = useState("");
  const mine = suggestMine && q.length === 0;
  const found = useQuery({
    queryKey: ["tickets", "picker", mine ? "mine" : q, companyId ?? null],
    queryFn: () =>
      call(
        api.GET("/api/v1/tickets", {
          params: {
            query: {
              limit: 15,
              ...(mine ? { view: "mine", sort: "updated_at" } : { q }),
              ...(companyId ? { company_id: companyId } : {}),
            },
          },
        }),
      ),
    enabled: mine || q.length > 0,
    staleTime: 30_000,
  });
  const options = (found.data?.items ?? [])
    .filter((t) => !exclude?.has(t.id))
    .map((t): PickerOption => ({ id: t.id, label: `${t.number} · ${t.subject}` }));
  return <Combobox {...props} options={options} loading={found.isFetching} onQueryChange={setQ} />;
}

/**
 * Finds an open Task by title as you type, optionally within one Company. With
 * `suggestMine`, an empty search lists the open Tasks assigned to the signed-in Analyst.
 */
export function TaskPicker({ companyId, exclude, suggestMine = false, ...props }: LinkPickerProps) {
  const [q, setQ] = useState("");
  const mine = suggestMine && q.length === 0;
  const found = useQuery({
    queryKey: ["tasks", "picker", mine ? "mine" : q],
    queryFn: () =>
      call(
        api.GET("/api/v1/tasks", {
          params: { query: { limit: 20, open: true, ...(mine ? { assignee: "me" } : { q }) } },
        }),
      ),
    enabled: mine || q.length > 0,
    staleTime: 30_000,
  });
  const options = (found.data?.items ?? [])
    .filter((k) => (!companyId || k.company_id === companyId) && !exclude?.has(k.id))
    .map((k): PickerOption => ({ id: k.id, label: k.title }));
  return <Combobox {...props} options={options} loading={found.isFetching} onQueryChange={setQ} />;
}
