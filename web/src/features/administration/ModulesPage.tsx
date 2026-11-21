import { useQuery } from "@tanstack/react-query";
import { Badge } from "@/components/Badge";
import { Card } from "@/components/Card";
import { Switch } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { SectionHeader } from "@/components/Page";
import { QueryState } from "@/components/QueryState";
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
    },
  );
  return (
    <>
      <SectionHeader title={t("admin.modules")} description={t("admin.modules.intro")} />
      <ProblemNotice error={toggle.error} />
      <QueryState query={modules}>
        {(list) => (
          <Card flush>
            <ul className="divide-y divide-line">
              {list.map((m) => (
                <li key={m.key} className="flex items-center justify-between gap-4 px-4 py-3">
                  <span className="flex flex-wrap items-center gap-2 font-medium">
                    {m.name}
                    {!m.can_disable ? <Badge tone="muted">{t("admin.modules.core")}</Badge> : null}
                  </span>
                  <Switch
                    label={t("admin.modules.toggle", { name: m.name })}
                    checked={m.is_enabled}
                    disabled={
                      !m.can_disable || (toggle.isPending && toggle.variables?.key === m.key)
                    }
                    onChange={(on) => toggle.mutate({ key: m.key, name: m.name, on })}
                    stateLabels={[t("common.on"), t("common.off")]}
                  />
                </li>
              ))}
            </ul>
          </Card>
        )}
      </QueryState>
    </>
  );
}
