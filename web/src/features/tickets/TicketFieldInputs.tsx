import { Checkbox, Fieldset, SelectField, TextAreaField, TextField } from "@/components/Field";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type FieldDef = Schemas["TicketFieldDto"];
export type FieldValues = Record<string, string | boolean>;

/** The fields that apply: active, and for the chosen Ticket Type (or every type). */
export function applicableFields(
  fields: readonly FieldDef[] | undefined,
  typeId?: string | null,
): FieldDef[] {
  return (fields ?? [])
    .filter((f) => f.is_active !== false)
    .filter((f) => !f.type_ids?.length || (typeId ? f.type_ids.includes(typeId) : false))
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

/** Answers as the API expects them: numbers as numbers, empty answers left out. */
export function fieldPayload(
  fields: readonly FieldDef[],
  values: FieldValues,
): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const field of fields) {
    const value = values[field.key];
    if (value === undefined || value === "") continue;
    out[field.key] =
      field.field_type === "number" && typeof value === "string" ? Number(value) : value;
  }
  return out;
}

/**
 * Inputs for administrator-defined Ticket Fields (FR-TKT-029), typed by field: text, long
 * text, number, date, dropdown and checkbox. The API validates the answers.
 */
export function TicketFieldInputs({
  fields,
  values,
  onChange,
  missing = [],
  legend,
}: {
  fields: readonly FieldDef[];
  values: FieldValues;
  onChange: (values: FieldValues) => void;
  /** Keys the API reported as required but unanswered. */
  missing?: readonly string[];
  legend: string;
}) {
  const t = useText();
  if (fields.length === 0) return null;
  const set = (key: string, value: string | boolean) => onChange({ ...values, [key]: value });
  return (
    <Fieldset legend={legend}>
      {fields.map((field) => {
        const value = values[field.key];
        const error = missing.includes(field.key) ? t("newTicket.fieldsRequired") : undefined;
        const common = {
          label: field.label,
          hint: field.help_text || undefined,
          required: Boolean(field.required_on_create),
          error,
        };
        switch (field.field_type) {
          case "checkbox":
            return (
              <Checkbox
                key={field.key}
                label={field.label}
                hint={field.help_text || undefined}
                checked={value === true}
                onChange={(e) => set(field.key, e.target.checked)}
              />
            );
          case "dropdown":
            return (
              <SelectField
                key={field.key}
                {...common}
                value={typeof value === "string" ? value : ""}
                placeholder={t("common.choose")}
                options={(field.options ?? []).map((o) => ({ value: o, label: o }))}
                onChange={(e) => set(field.key, e.target.value)}
              />
            );
          case "long_text":
            return (
              <TextAreaField
                key={field.key}
                {...common}
                value={typeof value === "string" ? value : ""}
                onChange={(e) => set(field.key, e.target.value)}
              />
            );
          default:
            return (
              <TextField
                key={field.key}
                {...common}
                type={
                  field.field_type === "number"
                    ? "number"
                    : field.field_type === "date"
                      ? "date"
                      : "text"
                }
                value={typeof value === "string" ? value : ""}
                onChange={(e) => set(field.key, e.target.value)}
              />
            );
        }
      })}
    </Fieldset>
  );
}
