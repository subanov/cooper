import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Button } from "@/components/Button";
import { ConfirmDialog } from "@/components/Dialog";
import { Notice, ProblemNotice, QueryState } from "@/components/Feedback";
import { TextAreaField } from "@/components/Field";
import { PageHeader, Panel } from "@/components/Layout";
import { fieldError, isFieldProblem, useAction } from "@/features/shared/mutation";
import { useFormat, useText } from "@/i18n/useText";
import { api, type Schemas } from "@/lib/api";
import { call } from "@/lib/request";

type Instructions = Schemas["SuggestedReplyInstructionsDto"];

const instructionsKey = ["administration", "suggested-reply-instructions"] as const;

const instructionsQuery = {
  queryKey: instructionsKey,
  queryFn: () => call(api.GET("/api/v1/suggested-reply-instructions")),
} as const;

/**
 * Suggested Reply Instructions (FR-AI-003): what the AI provider is told when an Analyst
 * asks for a Suggested Reply. Cooper's fixed rules are shown read-only: they are always
 * added and cannot be edited.
 */
export function SuggestedReplyInstructionsPage() {
  const t = useText();
  const instructions = useQuery(instructionsQuery);
  return (
    <>
      <PageHeader
        level={2}
        title={t("admin.suggestedReplyInstructions")}
        description={t("admin.suggestedReplyInstructions.intro")}
      />
      <QueryState query={instructions}>
        {(current) => <InstructionsForm current={current} />}
      </QueryState>
    </>
  );
}

function InstructionsForm({ current }: { current: Instructions }) {
  const t = useText();
  const f = useFormat();
  const [draft, setDraft] = useState(current.body);
  const [confirmReset, setConfirmReset] = useState(false);
  useEffect(() => setDraft(current.body), [current.body]);
  const save = useAction(
    (body: string) => call(api.PUT("/api/v1/suggested-reply-instructions", { body: { body } })),
    { invalidate: [instructionsKey], success: "admin.suggestedReplyInstructions.saved" },
  );
  const reset = useAction(() => call(api.DELETE("/api/v1/suggested-reply-instructions")), {
    invalidate: [instructionsKey],
    success: "admin.suggestedReplyInstructions.resetDone",
    onSuccess: () => setConfirmReset(false),
  });
  const dirty = draft !== current.body;
  return (
    <div className="grid gap-4">
      <Panel>
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate(draft);
          }}
        >
          <p className="text-sm text-fg-muted">
            {current.is_default || !current.updated_at
              ? t("admin.suggestedReplyInstructions.usingDefault")
              : t("admin.suggestedReplyInstructions.changed", {
                  at: f.dateTime(current.updated_at),
                })}
          </p>
          <TextAreaField
            label={t("admin.suggestedReplyInstructions.body")}
            hint={t("admin.suggestedReplyInstructions.placeholders", {
              placeholders: current.placeholders.join(", "),
              summary: "{ticket_summary}",
              max: f.number(current.max_chars),
            })}
            error={fieldError(save.error, "body")}
            required
            rows={12}
            maxLength={current.max_chars}
            className="font-mono"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
          />
          {isFieldProblem(save.error, ["body"]) ? null : <ProblemNotice error={save.error} />}
          <div className="flex flex-wrap gap-2">
            <Button type="submit" disabled={!dirty} loading={save.isPending}>
              {t("common.save")}
            </Button>
            <Button
              type="button"
              variant="secondary"
              disabled={!dirty}
              onClick={() => setDraft(current.body)}
            >
              {t("admin.suggestedReplyInstructions.discard")}
            </Button>
            <Button
              type="button"
              variant="secondary"
              disabled={current.is_default}
              onClick={() => setConfirmReset(true)}
            >
              {t("admin.suggestedReplyInstructions.reset")}
            </Button>
          </div>
        </form>
      </Panel>
      <Panel
        title={t("admin.suggestedReplyInstructions.fixedRules")}
        description={t("admin.suggestedReplyInstructions.fixedRulesIntro")}
      >
        <pre className="whitespace-pre-wrap text-sm text-fg-muted">{current.fixed_rules}</pre>
      </Panel>
      <Notice tone="info" live={false}>
        {t("admin.suggestedReplyInstructions.thread")}
      </Notice>
      <ConfirmDialog
        open={confirmReset}
        onOpenChange={setConfirmReset}
        title={t("admin.suggestedReplyInstructions.resetTitle")}
        confirmLabel={t("admin.suggestedReplyInstructions.reset")}
        busy={reset.isPending}
        onConfirm={() => reset.mutate(undefined)}
      >
        <p className="text-sm">{t("admin.suggestedReplyInstructions.resetBody")}</p>
        <ProblemNotice error={reset.error} />
      </ConfirmDialog>
    </div>
  );
}
