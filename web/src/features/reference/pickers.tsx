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
  return <Combobox {...props} options={options} loading={found.isFetching} onQueryChange={setQ} />;
}

/** Finds a ticket by number or subject as you type, optionally within one Company. */
export function TicketPicker({
  companyId,
  ...props
}: PickerProps & { companyId?: string | undefined }) {
  const [q, setQ] = useState("");
  const found = useQuery({
    queryKey: ["tickets", "picker", q, companyId ?? null],
    queryFn: () =>
      call(
        api.GET("/api/v1/tickets", {
          params: {
            query: { q, limit: 15, ...(companyId ? { company_id: companyId } : {}) },
          },
        }),
      ),
    enabled: q.length > 0,
    staleTime: 30_000,
  });
  const options = (found.data?.items ?? []).map(
    (t): PickerOption => ({
      id: t.id,
      label: `${t.number} · ${t.subject}`,
      description: undefined,
    }),
  );
  return <Combobox {...props} options={options} loading={found.isFetching} onQueryChange={setQ} />;
}
