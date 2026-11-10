import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { updateSetting } from "@/features/administration/actions";
import { constraintsOf, settingsQuery } from "@/features/administration/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";

type Setting = Schemas["SettingDto"];

/** System Settings, grouped by the prefix of their key (FR-SYS-001). */
export function SettingsPage() {
  const t = useText();
  const settings = useQuery(settingsQuery);
  if (settings.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (settings.isError) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  if (settings.data.length === 0) {
    return <p className="text-muted-foreground">{t("admin.settings.empty")}</p>;
  }
  const groups = new Map<string, Setting[]>();
  for (const setting of settings.data) {
    const group = setting.key.split(".")[0] ?? setting.key;
    groups.set(group, [...(groups.get(group) ?? []), setting]);
  }
  return (
    <div className="grid gap-6">
      <p className="text-muted-foreground">{t("admin.settings.intro")}</p>
      {[...groups].map(([group, items]) => (
        <section key={group} aria-labelledby={`group-${group}`} className="grid gap-3">
          <h2 id={`group-${group}`} className="text-lg font-semibold capitalize">
            {group.replaceAll("_", " ")}
          </h2>
          {items.map((setting) => (
            <SettingRow key={setting.key} setting={setting} />
          ))}
        </section>
      ))}
    </div>
  );
}

function SettingRow({ setting }: { setting: Setting }) {
  const t = useText();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState<string | boolean>(toDraft(setting.value));
  const [message, setMessage] = useState<{ tone: "success" | "error"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const constraints = constraintsOf(setting);
  const dirty = draft !== toDraft(setting.value);

  async function save() {
    setBusy(true);
    const outcome = await updateSetting(setting.key, fromDraft(setting.kind, draft));
    setBusy(false);
    if (outcome.ok) {
      setMessage({ tone: "success", text: t("admin.saved") });
      setDraft(toDraft(outcome.data.value));
      await queryClient.invalidateQueries({ queryKey: ["administration", "settings"] });
    } else if (outcome.problem.status === 403) {
      setMessage({ tone: "error", text: t("admin.forbidden") });
    } else {
      setMessage({
        tone: "error",
        text: t("admin.invalid", { detail: outcome.problem.detail ?? outcome.problem.kind }),
      });
    }
  }

  const id = `setting-${setting.key}`;
  return (
    <form
      noValidate
      className="grid gap-2 rounded-lg border border-border p-4"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      {setting.kind === "bool" ? (
        <div className="flex items-center gap-2">
          <input
            id={id}
            type="checkbox"
            checked={draft === true}
            onChange={(event) => setDraft(event.target.checked)}
          />
          <Label htmlFor={id}>{setting.key}</Label>
        </div>
      ) : setting.kind === "choice" ? (
        <div className="grid gap-1.5">
          <Label htmlFor={id}>{setting.key}</Label>
          <Select id={id} value={String(draft)} onChange={(e) => setDraft(e.target.value)}>
            {(constraints.choices ?? []).map((choice) => (
              <option key={choice} value={choice}>
                {choice}
              </option>
            ))}
          </Select>
        </div>
      ) : (
        <Field
          id={id}
          label={setting.key}
          type={setting.kind === "integer" ? "number" : "text"}
          value={String(draft)}
          min={constraints.min}
          max={constraints.max}
          maxLength={constraints.max_chars}
          onChange={(event) => setDraft(event.target.value)}
        />
      )}
      <p className="text-sm text-muted-foreground">{setting.description}</p>
      <p className="text-xs text-muted-foreground">
        {t("admin.default", { value: String(setting.default) })}
      </p>
      <div className="flex items-center gap-3">
        <Button type="submit" size="sm" disabled={!dirty || busy}>
          {t("admin.save")}
        </Button>
        {message ? (
          <span role={message.tone === "error" ? "alert" : "status"} className="text-sm">
            {message.text}
          </span>
        ) : null}
      </div>
    </form>
  );
}

function toDraft(value: unknown): string | boolean {
  return typeof value === "boolean" ? value : String(value ?? "");
}

function fromDraft(kind: string, draft: string | boolean): unknown {
  return kind === "integer" ? Number(draft) : draft;
}
