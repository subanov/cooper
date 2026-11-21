import { useQuery } from "@tanstack/react-query";
import { Badge } from "@/components/Badge";
import { ProblemNotice, QueryState } from "@/components/Feedback";
import { Switch } from "@/components/Field";
import { PageHeader, Panel } from "@/components/Layout";
import { modulesQuery } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";

/** Module switches (FR-GEN-005). A Module that is off is hidden from navigation. */
export function ModulesPage() {
  const t = useText();
  const modules = useQuery(modulesQuery);
  const toggle = useAction(
    (m: { key: string; name: string; on: boolean }) =>
      call(
        api.PUT("/api/v1/modules/{key}", {
          params: { path: { key: m.key } },
          body: { is_enabled: m.on },
        }),
      ),
    {
      invalidate: [["modules"]],
      success: (_, m) => t(m.on ? "admin.modules.on" : "admin.modules.off", { name: m.name }),
      undo: (_, m) => ({
        label: t("common.undo"),
        run: (): void => toggle.mutate({ ...m, on: !m.on }),
      }),
    },
  );
  return (
    <>
      <PageHeader level={2} title={t("admin.modules")} description={t("admin.modules.intro")} />
      <ProblemNotice error={toggle.error} />
      <QueryState query={modules}>
        {(list) => (
          <Panel flush>
            <ul className="divide-y divide-line">
              {list.map((m) => (
                <li key={m.key} className="flex items-center justify-between gap-4 px-4 py-3">
                  <span className="flex flex-wrap items-center gap-2 font-medium">
                    {m.name}
                    {!m.can_disable ? <Badge tone="muted">{t("admin.modules.core")}</Badge> : null}
                  </span>
                  <span className="flex items-center gap-2 text-xs text-fg-muted">
                    <span aria-hidden="true">
                      {m.is_enabled ? t("common.on") : t("common.off")}
                    </span>
                    <Switch
                      label={t("admin.modules.toggle", { name: m.name })}
                      checked={m.is_enabled}
                      disabled={
                        !m.can_disable || (toggle.isPending && toggle.variables?.key === m.key)
                      }
                      onCheckedChange={(on) => toggle.mutate({ key: m.key, name: m.name, on })}
                    />
                  </span>
                </li>
              ))}
            </ul>
          </Panel>
        )}
      </QueryState>
    </>
  );
}
