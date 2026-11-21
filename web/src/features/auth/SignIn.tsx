import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { KeyRound } from "lucide-react";
import { type FormEvent, useState } from "react";
import { Button, buttonClass } from "@/components/Button";
import { Notice } from "@/components/Feedback";
import { Checkbox, TextField } from "@/components/Field";
import { analystSessionQuery } from "@/features/session/session";
import { PublicLayout } from "@/features/shell/PublicLayout";
import { useText } from "@/i18n/useText";
import { api, setCsrfToken } from "@/lib/api";
import { type Problem, problemOf } from "@/lib/problem";
import { call } from "@/lib/request";
import { safeRedirect } from "@/lib/safeRedirect";

/** Analyst sign-in with an optional two-factor step and single sign-on (FR-IAM-002/003). */
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
  const target = safeRedirect(redirect);
  const sso = useQuery({
    queryKey: ["sso", "analysts"],
    queryFn: () =>
      call(api.GET("/api/v1/sso/providers", { params: { query: { audience: "analysts" } } })),
    staleTime: 5 * 60_000,
  });
  const setup = useQuery({
    queryKey: ["setup"],
    queryFn: () => call(api.GET("/api/v1/setup")),
    staleTime: 5 * 60_000,
  });

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setProblem(null);
    try {
      const session = await call(
        api.POST("/api/v1/session", {
          body: {
            login: login.trim(),
            password,
            totp_code: needsCode ? code.replace(/\s/g, "") : null,
            remember_device: needsCode && remember,
          },
        }),
      );
      setCsrfToken("analyst", session.csrf_token);
      queryClient.setQueryData(analystSessionQuery.queryKey, session);
      await navigate({ to: target });
    } catch (error) {
      const p = problemOf(error);
      if (p.kind === "totp-required") setNeedsCode(true);
      setProblem(p);
    } finally {
      setBusy(false);
    }
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
        return t("signIn.banned", { minutes: Math.ceil((problem?.retryAfter ?? 60) / 60) });
      default:
        return problem?.detail ?? t("problem.tryAgain");
    }
  })();

  return (
    <PublicLayout
      title={t("signIn.title")}
      intro={t("signIn.intro")}
      footer={
        <Link to="/portal" className="text-accent-text hover:underline">
          {t("signIn.portal")}
        </Link>
      }
    >
      {setup.data?.required ? (
        <Notice tone="info" title={t("signIn.setupNeeded")}>
          <Link to="/setup" className="font-medium text-accent-text underline">
            {t("signIn.setupLink")}
          </Link>
        </Notice>
      ) : null}
      <form className="grid gap-4" onSubmit={submit}>
        {message ? (
          <Notice tone={problem?.kind === "totp-required" ? "info" : "danger"}>{message}</Notice>
        ) : null}
        <TextField
          label={t("signIn.login")}
          name="username"
          autoComplete="username"
          required
          autoFocus
          value={login}
          readOnly={needsCode}
          onChange={(e) => setLogin(e.target.value)}
        />
        <TextField
          label={t("signIn.password")}
          name="password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          readOnly={needsCode}
          onChange={(e) => setPassword(e.target.value)}
        />
        {needsCode ? (
          <>
            <TextField
              label={t("signIn.code")}
              hint={t("signIn.codeHint")}
              name="totp"
              inputMode="numeric"
              autoComplete="one-time-code"
              required
              autoFocus
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
            <Checkbox
              label={t("signIn.remember")}
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
          </>
        ) : null}
        <Button type="submit" variant="primary" size="lg" loading={busy}>
          {needsCode ? t("signIn.verify") : t("signIn.submit")}
        </Button>
        <div className="flex flex-wrap justify-between gap-2 text-sm">
          <Link to="/reset-password" className="text-accent-text hover:underline">
            {t("signIn.forgot")}
          </Link>
          {needsCode ? (
            <button
              type="button"
              className="text-accent-text hover:underline"
              onClick={() => {
                setNeedsCode(false);
                setCode("");
                setPassword("");
                setProblem(null);
              }}
            >
              {t("signIn.startOver")}
            </button>
          ) : null}
        </div>
      </form>
      {sso.data && sso.data.length > 0 && !needsCode ? (
        <div className="grid gap-3">
          <p className="flex items-center gap-3 text-xs text-fg-muted before:h-px before:flex-1 before:bg-line after:h-px after:flex-1 after:bg-line">
            {t("signIn.or")}
          </p>
          {sso.data.map((provider) => (
            <a
              key={provider.id}
              href={`/api/v1/sso/${provider.id}/start?audience=analysts&return_to=${encodeURIComponent(target)}`}
              className={buttonClass({ variant: "secondary", size: "lg" })}
            >
              <KeyRound aria-hidden="true" />
              {t("signIn.sso", { provider: provider.display_name })}
            </a>
          ))}
        </div>
      ) : null}
    </PublicLayout>
  );
}
