import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "@/components/Button";
import { CheckboxList } from "@/components/CheckboxList";
import { Checkbox } from "@/components/Field";
import { companiesQuery } from "@/features/reference/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type Access = Schemas["CompanyAccessDto"];

/** Every Company, or a chosen set (for Analysts and Teams; FR-GEN-002). */
export function CompanyAccessEditor({
  value,
  onSave,
  busy,
  disabled,
}: {
  value: Access;
  onSave: (access: Access) => void;
  busy: boolean;
  disabled?: boolean | undefined;
}) {
  const t = useText();
  const companies = useQuery(companiesQuery);
  const [all, setAll] = useState(value.all);
  const [ids, setIds] = useState<string[]>(value.company_ids ?? []);
  const dirty =
    all !== value.all ||
    JSON.stringify([...ids].sort()) !== JSON.stringify([...(value.company_ids ?? [])].sort());
  return (
    <form
      className="grid gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(all ? { all: true } : { all: false, company_ids: ids });
      }}
    >
      <Checkbox
        label={t("users.allCompanies")}
        checked={all}
        disabled={disabled}
        onChange={(e) => setAll(e.target.checked)}
      />
      {!all ? (
        <CheckboxList
          legend={t("users.companyAccess")}
          options={(companies.data ?? []).map((c) => ({
            value: c.id,
            label: c.name,
            description: c.is_active ? undefined : t("common.inactive"),
          }))}
          value={ids}
          onChange={setIds}
          disabled={disabled}
        />
      ) : null}
      <div>
        <Button type="submit" disabled={!dirty || disabled} loading={busy}>
          {t("users.saveAccess")}
        </Button>
      </div>
    </form>
  );
}
