import { useQuery } from "@tanstack/react-query";
import { type Ref, useEffect, useImperativeHandle, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { Checkbox, Input, Select, Textarea } from "@/components/Field";
import { ProblemNotice } from "@/components/Notice";
import { Segmented } from "@/components/Tabs";
import { useToast } from "@/components/Toast";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";
import { replyTemplatesQuery, ticketActions } from "./api";

export interface ComposerHandle {
  focus: (mode: "reply" | "note") => void;
}

/**
 * Writes a Reply (emailed to the Requester) or a Note (internal unless made public).
 * Internal Notes are tinted so they are never mistaken for a reply (H5). Ctrl+Enter
 * sends; the text survives errors (H9).
 */
export function Composer({
  ticketId,
  requesterName,
  hasRequester,
  ref,
  onComposingChange,
  disabled,
}: {
  ticketId: string;
  requesterName?: string | undefined;
  hasRequester: boolean;
  ref?: Ref<ComposerHandle> | undefined;
  onComposingChange?: ((composing: boolean) => void) | undefined;
  disabled?: boolean | undefined;
}) {
  const t = useText();
  const [mode, setMode] = useState<"reply" | "note">(hasRequester ? "reply" : "note");
  const [body, setBody] = useState("");
  const [cc, setCc] = useState("");
  const [isPublic, setIsPublic] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const toast = useToast();
  const templates = useQuery({ ...replyTemplatesQuery, enabled: mode === "reply" });
  const hasText = body.trim() !== "";
  useEffect(() => onComposingChange?.(hasText), [hasText, onComposingChange]);
  useImperativeHandle(ref, () => ({
    focus: (next) => {
      setMode(next === "reply" && !hasRequester ? "note" : next);
      requestAnimationFrame(() => area.current?.focus());
    },
  }));

  const send = useAction<void, unknown>(
    () =>
      mode === "reply"
        ? ticketActions.reply(ticketId, {
            body,
            cc: cc
              .split(/[,;\s]+/)
              .map((a) => a.trim())
              .filter(Boolean),
          })
        : ticketActions.note(ticketId, { body, is_public: isPublic }),
    {
      invalidate: [
        ["tickets", "detail", ticketId],
        ["tickets", "list"],
      ],
      success: () => t(mode === "reply" ? "composer.sent" : "composer.noted"),
      onSuccess: () => {
        setBody("");
        setCc("");
        setIsPublic(false);
      },
    },
  );

  async function insertTemplate(templateId: string) {
    if (!templateId) return;
    const rendered = await ticketActions
      .renderTemplate(ticketId, templateId)
      .catch(() => undefined);
    if (rendered) {
      setBody((current) => (current.trim() ? `${current}\n\n${rendered.text}` : rendered.text));
      area.current?.focus();
    }
  }

  const label =
    mode === "reply"
      ? requesterName
        ? t("composer.replyLabel", { name: requesterName })
        : t("composer.replyLabelNoName")
      : t("composer.noteLabel");
  const internal = mode === "note" && !isPublic;

  return (
    <section
      aria-labelledby="composer-title"
      className={cn(
        "grid gap-3 rounded-lg border bg-surface p-3 shadow-raised",
        internal ? "border-warning-text/40" : "border-line",
      )}
    >
      <h2 id="composer-title" className="sr-only">
        {t("composer.title")}
      </h2>
      <div className="flex flex-wrap items-center justify-between gap-2">
        {hasRequester ? (
          <Segmented
            label={t("composer.mode")}
            value={mode}
            onChange={setMode}
            options={[
              { value: "reply" as const, label: t("composer.reply") },
              { value: "note" as const, label: t("composer.note") },
            ]}
          />
        ) : (
          <p className="text-[0.8125rem] font-semibold">{t("composer.note")}</p>
        )}
        {mode === "reply" && (templates.data ?? []).length > 0 ? (
          <Select
            aria-label={t("composer.template")}
            value=""
            onChange={(e) => void insertTemplate(e.target.value)}
            className="h-8 w-auto"
          >
            <option value="">{t("composer.templateNone")}</option>
            {(templates.data ?? [])
              .filter((tpl) => tpl.id && tpl.is_active !== false)
              .map((tpl) => (
                <option key={tpl.id} value={tpl.id ?? ""}>
                  {tpl.name}
                </option>
              ))}
          </Select>
        ) : null}
      </div>
      {!hasRequester ? (
        <p className="-mt-1 text-xs text-fg-muted">{t("composer.noRequester")}</p>
      ) : null}
      <form
        className="grid gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (hasText) send.mutate();
        }}
      >
        <label htmlFor="composer-body" className="sr-only">
          {label}
        </label>
        <Textarea
          ref={area}
          id="composer-body"
          aria-describedby="composer-hint"
          placeholder={label}
          value={body}
          disabled={disabled}
          rows={5}
          onChange={(e) => setBody(e.target.value)}
          onKeyDown={(e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === "Enter" && hasText) {
              e.preventDefault();
              send.mutate();
            }
          }}
          className={internal ? "bg-warning-soft/40" : undefined}
        />
        {mode === "reply" ? (
          <Input
            aria-label={t("composer.cc")}
            aria-describedby="composer-cc-hint"
            placeholder={`${t("composer.cc")} · ${t("composer.ccHint")}`}
            value={cc}
            onChange={(e) => setCc(e.target.value)}
            className="h-8"
          />
        ) : null}
        <span id="composer-cc-hint" className="sr-only">
          {t("composer.ccHint")}
        </span>
        <ProblemNotice error={send.error} />
        <div className="flex flex-wrap items-center gap-3">
          {mode === "note" ? (
            <Checkbox
              label={t("composer.public")}
              checked={isPublic}
              onChange={(e) => setIsPublic(e.target.checked)}
            />
          ) : null}
          <p id="composer-hint" className="text-xs text-fg-muted">
            {mode === "reply" ? t("composer.replyHint") : t("composer.noteHint")}
          </p>
          <div className="ml-auto flex gap-2">
            {hasText ? (
              <Button
                variant="ghost"
                onClick={() => {
                  // Discarding is undoable, so it needs no confirmation (H3).
                  const previous = body;
                  setBody("");
                  toast({
                    tone: "info",
                    message: t("composer.discarded"),
                    action: { label: t("common.undo"), onClick: () => setBody(previous) },
                  });
                }}
              >
                {t("composer.discard")}
              </Button>
            ) : null}
            <Button
              type="submit"
              variant="primary"
              loading={send.isPending}
              disabled={!hasText || disabled}
            >
              {mode === "reply" ? t("composer.send") : t("composer.addNote")}
            </Button>
          </div>
        </div>
      </form>
    </section>
  );
}
