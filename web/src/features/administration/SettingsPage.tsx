import { useQuery } from "@tanstack/react-query";
import { useId, useState } from "react";
import { Button } from "@/components/Button";
import { Card } from "@/components/Card";
import { Input, Select, Switch } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { QueryState } from "@/components/QueryState";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

type Setting = Schemas["SettingDto"];

const settingsQuery = {
  queryKey: ["administration", "settings"],
  queryFn: () => call(api.GET("/api/v1/settings")),
} as const;

/** System Settings, grouped by the first part of their key (FR-SYS-001). */
export function SettingsPage() {
  const t = useText();
  const settings = useQuery(settingsQuery);
  const [filter, setFilter] = useState("");
  return (
    <>
      <SectionHeader title={t("admin.settings")} description={t("admin.settings.intro")} />
      <Input
        type="search"
        aria-label={t("admin.settings.search")}
        placeholder={t("admin.settings.search")}
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        className="max-w-sm"
      />
      <QueryState query={settings}>
        {(list) => {
          const q = filter.trim().toLowerCase();
          const shown = list.filter(
            (s) => !q || `${s.key} ${s.description}`.toLowerCase().includes(q),
          );
          if (list.length === 0)
            return <p className="text-sm text-fg-muted">{t("admin.settings.empty")}</p>;
          const groups = new Map<string, Setting[]>();
          for (const s of shown) {
            const group = s.key.split(".")[0] ?? s.key;
            groups.set(group, [...(groups.get(group) ?? []), s]);
          }
          return [...groups].map(([group, items]) => (
            <Card
              key={group}
              title={<span className="capitalize">{group.replaceAll("_", " ")}</span>}
              flush
            >
              <ul className="divide-y divide-line">
                {items.map((s) => (
                  <li key={s.key}>
                    <SettingRow setting={s} />
                  </li>
                ))}
              </ul>
            </Card>
          ));
        }}
      </QueryState>
    </>
  );
}

function asText(value: unknown): string {
  return typeof value === "string"
    ? value
    : value === null || value === undefined
      ? ""
      : String(value);
}

function SettingRow({ setting }: { setting: Setting }) {
  const t = useText();
  const id = useId();
  const constraints = setting.constraints as {
    min?: number;
    max?: number;
    max_chars?: number;
    choices?: string[];
  };
  const [draft, setDraft] = useState<string>(asText(setting.value));
  const save = useAction(
    (value: unknown) =>
      call(
        api.PUT("/api/v1/settings/{key}", {
          params: { path: { key: setting.key } },
          body: { value: value as never },
        }),
      ),
    {
      invalidate: [["administration", "settings"]],
      success: () => t("admin.settings.saved", { key: setting.key }),
    },
  );
  const dirty = draft !== asText(setting.value);
  const about = (
    <div className="grid gap-0.5">
      <label htmlFor={id} className="font-mono text-[0.8125rem] font-medium">
        {setting.key}
      </label>
      <p id={`${id}-about`} className="text-xs text-fg-muted">
        {setting.description}{" "}
        {t("admin.settings.default", { value: asText(setting.default) || "—" })}
      </p>
    </div>
  );
  if (setting.kind === "bool") {
    return (
      <div className="grid gap-2 px-4 py-3">
        <div className="flex items-start justify-between gap-4">
          {about}
          <Switch
            id={id}
            describedBy={`${id}-about`}
            checked={setting.value === (true as never)}
            disabled={save.isPending}
            onChange={(on) => save.mutate(on)}
            stateLabels={[t("common.on"), t("common.off")]}
          />
        </div>
        <ProblemNotice error={save.error} />
      </div>
    );
  }
  return (
    <form
      className="grid gap-2 px-4 py-3 md:grid-cols-[minmax(0,1fr)_22rem] md:items-start md:gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate(setting.kind === "integer" ? Number(draft) : draft);
      }}
    >
      {about}
      <div className="flex gap-2">
        {setting.kind === "choice" ? (
          <Select
            id={id}
            aria-describedby={`${id}-about`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          >
            {(constraints.choices ?? []).map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </Select>
        ) : (
          <Input
            id={id}
            aria-describedby={`${id}-about`}
            type={setting.kind === "integer" ? "number" : "text"}
            min={constraints.min}
            max={constraints.max}
            maxLength={constraints.max_chars}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
        )}
        <Button
          type="submit"
          variant={dirty ? "primary" : "secondary"}
          disabled={!dirty}
          loading={save.isPending}
        >
          {t("common.save")}
        </Button>
      </div>
      <ProblemNotice error={save.error} className="md:col-span-2" />
    </form>
  );
}
