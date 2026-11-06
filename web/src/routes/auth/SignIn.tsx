import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { signIn } from "@/features/session/actions";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Problem } from "@/lib/problem";
import { PublicShell } from "../PublicShell";
import { safeRedirect } from "./safeRedirect";

/** Analyst sign-in with an optional two-factor step (FR-IAM-002, FR-IAM-003). */
export function SignInPage({ redirect }: { redirect: string | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [login, setLogin] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [needsCode, setNeedsCode] = useState(false);
  const [remember, setRemember] = useState(false);
  const [problem, setProblem] = useState<Problem | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setProblem(null);
    const result = await signIn({
      login,
      password,
      totp_code: needsCode ? code : null,
      remember_device: needsCode && remember,
    });
    setBusy(false);
    if (result.ok) {
      queryClient.setQueryData(analystSessionQuery.queryKey, result.data);
      await navigate({ to: safeRedirect(redirect) });
      return;
    }
    if (result.problem.kind === "totp-required") {
      setNeedsCode(true);
    }
    setProblem(result.problem);
  }

  const message = (() => {
    switch (problem?.kind) {
      case undefined:
        return null;
      case "invalid-credentials":
        return t("signIn.invalid");
      case "totp-required":
        return t("signIn.codeNeeded");
      case "totp-invalid":
        return t("signIn.codeWrong");
      case "too-many-sign-in-attempts":
        return t("signIn.banned", { minutes: Math.ceil((problem.retryAfter ?? 60) / 60) });
      default:
        return t("common.error");
    }
  })();

  return (
    <PublicShell title={t("signIn.title")}>
      <form className="grid gap-4" onSubmit={submit} noValidate={false}>
        {message ? (
          <Alert variant={problem?.kind === "totp-required" ? "info" : "error"}>{message}</Alert>
        ) : null}
        <Field
          label={t("signIn.login")}
          name="username"
          autoComplete="username"
          required
          value={login}
          onChange={(e) => setLogin(e.target.value)}
          readOnly={needsCode}
        />
        <Field
          label={t("signIn.password")}
          name="password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          readOnly={needsCode}
        />
        {needsCode ? (
          <>
            <Field
              label={t("signIn.code")}
              hint={t("signIn.codeHint")}
              name="totp"
              inputMode="numeric"
              autoComplete="one-time-code"
              pattern="[0-9 ]*"
              required
              autoFocus
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={remember}
                onChange={(e) => setRemember(e.target.checked)}
              />
              {t("signIn.remember")}
            </label>
          </>
        ) : null}
        <Button type="submit" disabled={busy}>
          {busy ? t("signIn.submitting") : t("signIn.submit")}
        </Button>
        <div className="flex flex-col gap-1 text-sm">
          <Link to="/reset-password" className="text-primary underline-offset-4 hover:underline">
            {t("signIn.forgot")}
          </Link>
          <Link to="/portal" className="text-muted-foreground underline-offset-4 hover:underline">
            {t("signIn.portal")}
          </Link>
        </div>
      </form>
    </PublicShell>
  );
}
