import { useNavigate } from "@tanstack/react-router";
import { Search } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { Checkbox, Input } from "@/components/Field";
import { FilterBar } from "@/components/Page";
import { useText } from "@/i18n/useText";

export interface PeopleSearch {
  q?: string;
  inactive?: true;
}

/** Search and "Show deactivated" for a list of people, kept in the URL. */
export function PeopleFilters({
  to,
  search,
}: {
  to: "/users/analysts" | "/users/requesters";
  search: PeopleSearch;
}) {
  const t = useText();
  const navigate = useNavigate();
  const [q, setQ] = useState(search.q ?? "");
  useEffect(() => setQ(search.q ?? ""), [search.q]);
  return (
    <FilterBar label={t("common.filters")}>
      <form
        className="relative w-full max-w-xs"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void navigate({ to, search: { ...search, q: q.trim() || undefined } });
        }}
      >
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-fg-muted"
        />
        <Input
          type="search"
          aria-label={t("users.search")}
          placeholder={t("users.searchPlaceholder")}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          className="pl-8"
        />
      </form>
      <Checkbox
        className="h-9 items-center"
        label={t("users.includeInactive")}
        checked={Boolean(search.inactive)}
        onChange={(e) =>
          void navigate({
            to,
            search: { ...search, inactive: e.target.checked ? true : undefined },
          })
        }
      />
    </FilterBar>
  );
}
