import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/Button";
import { LoadingBlock, Notice, ProblemNotice } from "@/components/Feedback";
import { TextField } from "@/components/Field";
import { analystSessionQuery } from "@/features/session/session";
import { PublicLayout } from "@/features/shell/PublicLayout";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { problemOf } from "@/lib/problem";
import { call } from "@/lib/request";

/** The authenticator key and code check, shared by required setup and My account. */
export function TotpEnrollment({ onDone }: { onDone: () => void }) {
  const t = useText();
  const [code, setCode] = useState("");
  const begin = useMutation({
    mutationFn: () => call(api.POST("/api/v1/session/totp/enrollment")),
  });
  const confirm = useMutation({
    mutationFn: () =>
      call(
        api.POST("/api/v1/session/totp/enrollment/confirm", {
          body: { code: code.replace(/\s/g, "") },
        }),
      ),
    onSuccess: onDone,
  });
  const { mutate: start } = begin;
  const started = useRef(false);
  useEffect(() => {
    // One enrollment per visit: each call issues a new secret.
    if (started.current) return;
    started.current = true;
    start();
  }, [start]);

  if (begin.isPending || begin.isIdle) return <LoadingBlock />;
  if (begin.isError) return <ProblemNotice error={begin.error} />;
  const secret = begin.data;
  const wrong = confirm.isError && problemOf(confirm.error).status === 422;
  return (
    <form
      className="grid gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        confirm.mutate();
      }}
    >
      <ol className="grid gap-5 text-sm">
        <li className="grid grid-cols-[1.75rem_1fr] gap-3">
          <span
            aria-hidden="true"
            className="grid size-7 place-items-center rounded-full bg-accent-soft text-xs font-semibold text-accent-text"
          >
            1
          </span>
          <div className="grid gap-2">
            <p>{t("totp.step1")}</p>
            <p className="text-xs text-fg-muted">{t("totp.secret")}</p>
            <code className="select-all break-all rounded-md border border-line bg-sunken px-3 py-2 font-mono text-sm tracking-wider">
              {secret.secret.replace(/(.{4})/g, "$1 ").trim()}
            </code>
            <a href={secret.otpauth_uri} className="w-fit text-accent-text hover:underline">
              {t("totp.openApp")}
            </a>
          </div>
        </li>
        <li className="grid grid-cols-[1.75rem_1fr] gap-3">
          <span
            aria-hidden="true"
            className="grid size-7 place-items-center rounded-full bg-accent-soft text-xs font-semibold text-accent-text"
          >
            2
          </span>
          <div className="grid gap-2">
            <p>{t("totp.step2")}</p>
            <TextField
              label={t("totp.code")}
              inputMode="numeric"
              autoComplete="one-time-code"
              required
              value={code}
              error={wrong ? t("totp.wrong") : undefined}
              onChange={(e) => setCode(e.target.value)}
            />
          </div>
        </li>
      </ol>
      {confirm.isError && !wrong ? <ProblemNotice error={confirm.error} /> : null}
      <Button
        type="submit"
        variant="primary"
        size="lg"
        loading={confirm.isPending}
        disabled={!code.trim()}
      >
        {t("totp.confirm")}
      </Button>
    </form>
  );
}

/** Required two-factor setup before an Analyst may continue (FR-IAM-003). */
export function TwoFactorSetupPage() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  return (
    <PublicLayout title={t("totp.title")}>
      <Notice tone="info">{t("totp.required")}</Notice>
      <TotpEnrollment
        onDone={async () => {
          await queryClient.invalidateQueries({ queryKey: analystSessionQuery.queryKey });
          await navigate({ to: "/" });
        }}
      />
    </PublicLayout>
  );
}
