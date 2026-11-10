import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { setModule } from "@/features/administration/actions";
import { modulesQuery } from "@/features/navigation/modules";
import { useText } from "@/i18n/useText";

/** Turns Modules on or off (FR-SYS-001, FR-GEN-005). */
export function ModulesPage() {
  const t = useText();
  const queryClient = useQueryClient();
  const modules = useQuery(modulesQuery);
  const [error, setError] = useState<string | null>(null);
  const toggle = useMutation({
    mutationFn: async ({ key, enabled }: { key: string; enabled: boolean }) => {
      const outcome = await setModule(key, enabled);
      if (!outcome.ok) {
        throw new Error(outcome.problem.detail ?? outcome.problem.kind);
      }
    },
    onSuccess: async () => {
      setError(null);
      await queryClient.invalidateQueries({ queryKey: modulesQuery.queryKey });
    },
    onError: (e: Error) => setError(t("admin.invalid", { detail: e.message })),
  });

  if (modules.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (modules.isError) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid gap-3">
      <p className="text-muted-foreground">{t("admin.modules.intro")}</p>
      {error ? <Alert variant="error">{error}</Alert> : null}
      <ul className="grid gap-2">
        {modules.data.map((module) => (
          <li
            key={module.key}
            className="flex items-center gap-3 rounded-lg border border-border px-4 py-3"
          >
            <span className="font-medium">{module.name}</span>
            <Badge tone={module.is_enabled ? "ok" : "muted"}>
              {t(module.is_enabled ? "admin.modules.on" : "admin.modules.off")}
            </Badge>
            {module.can_disable ? (
              <Button
                className="ml-auto"
                size="sm"
                variant="outline"
                disabled={toggle.isPending}
                onClick={() => toggle.mutate({ key: module.key, enabled: !module.is_enabled })}
              >
                {t(module.is_enabled ? "admin.modules.turnOff" : "admin.modules.turnOn", {
                  name: module.name,
                })}
              </Button>
            ) : (
              <span className="ml-auto text-sm text-muted-foreground">
                {t("admin.modules.core")}
              </span>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}
