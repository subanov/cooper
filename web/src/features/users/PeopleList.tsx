import { useNavigate } from "@tanstack/react-router";
import { Checkbox } from "@/components/Field";
import { SearchBox } from "@/components/ListSearch";
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
  return (
    <>
      <SearchBox
        label={t("users.search")}
        placeholder={t("users.searchPlaceholder")}
        value={search.q}
        onSubmit={(q) => void navigate({ to, search: { ...search, q } })}
      />
      <Checkbox
        label={t("users.includeInactive")}
        checked={Boolean(search.inactive)}
        onChange={(e) =>
          void navigate({
            to,
            search: { ...search, inactive: e.target.checked ? true : undefined },
          })
        }
      />
    </>
  );
}
