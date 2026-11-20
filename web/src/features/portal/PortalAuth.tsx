import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { KeyRound } from "lucide-react";
import { type FormEvent, useState } from "react";
import { Button, buttonClass } from "@/components/Button";
import { TextField } from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { NewPasswordForm } from "@/features/auth/ResetPassword";
import { portalSessionQuery } from "@/features/session/session";
import { PublicLayout } from "@/features/shell/PublicLayout";
import { useText } from "@/i18n/useText";
import { api, setCsrfToken } from "@/lib/api";
import { problemOf } from "@/lib/problem";
import { call } from "@/lib/request";
import { safeRedirect } from "@/lib/safeRedirect";

function BackToSignIn() {
  const t = useText();
  return (
    <Link to="/portal/sign-in" className="text-accent-text hover:underline">
      {t("portal.backToSignIn")}
    </Link>
  );
}

/** Portal sign-in (FR-PRT-001): password, single sign-on, or a link to register. */
export function PortalSignInPage({ redirect }: { redirect?: string | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const target = safeRedirect(redirect, "portal");
  const options = useQuery({
    queryKey: ["portal", "sign-in-options"],
    queryFn: () => call(api.GET("/api/v1/portal/sign-in-options")),
    staleTime: 5 * 60_000,
  });
  const sso = useQuery({
    queryKey: ["sso", "requesters"],
    queryFn: () =>
      call(api.GET("/api/v1/sso/providers", { params: { query: { audience: "requesters" } } })),
    staleTime: 5 * 60_000,
  });
  const signIn = useMutation({
    mutationFn: () =>
      call(api.POST("/api/v1/portal/session", { body: { email: email.trim(), password } })),
    onSuccess: async (session) => {
      setCsrfToken("portal", session.csrf_token);
      queryClient.setQueryData(portalSessionQuery.queryKey, session);
      await navigate({ to: target });
    },
  });
  const problem = signIn.error ? problemOf(signIn.error) : null;
  return (
    <PublicLayout
      area="portal"
      title={t("portalSignIn.title")}
      intro={t("portalSignIn.intro")}
      footer={
        <Link to="/sign-in" className="text-fg-muted hover:underline">
          {t("portalSignIn.analyst")}
        </Link>
      }
    >
      <form
        className="grid gap-4"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          signIn.mutate();
        }}
      >
        {problem ? (
          problem.status === 401 || problem.kind === "invalid-credentials" ? (
            <Notice tone="danger">{t("portalSignIn.invalid")}</Notice>
          ) : (
            <ProblemNotice error={signIn.error} />
          )
        ) : null}
        <TextField
          label={t("portalSignIn.email")}
          type="email"
          autoComplete="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <TextField
          label={t("portalSignIn.password")}
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Button type="submit" variant="primary" size="lg" loading={signIn.isPending}>
          {t("portalSignIn.submit")}
        </Button>
        <div className="grid gap-1.5 text-sm">
          <Link to="/portal/reset-password" className="text-accent-text hover:underline">
            {t("portalSignIn.forgot")}
          </Link>
          {options.data?.self_registration ? (
            <Link to="/portal/register" className="text-accent-text hover:underline">
              {t("portalSignIn.register")}
            </Link>
          ) : null}
        </div>
      </form>
      {(sso.data ?? []).map((p) => (
        <a
          key={p.id}
          href={`/api/v1/sso/${p.id}/start?audience=requesters&return_to=${encodeURIComponent(target)}`}
          className={buttonClass({ size: "lg" })}
        >
          <KeyRound aria-hidden="true" className="size-4" />
          {t("portalSignIn.sso", { provider: p.display_name })}
        </a>
      ))}
    </PublicLayout>
  );
}

/** Asks for a link to set a Portal password. Never reveals whether the address is known. */
export function PortalResetPage() {
  const t = useText();
  const [email, setEmail] = useState("");
  const request = useMutation({
    mutationFn: () =>
      call(api.POST("/api/v1/portal/password-reset", { body: { email: email.trim() } })),
  });
  return (
    <PublicLayout
      area="portal"
      title={t("portalReset.title")}
      intro={t("portalReset.intro")}
      footer={<BackToSignIn />}
    >
      {request.isSuccess ? (
        <Notice tone="success">{t("portalReset.sent")}</Notice>
      ) : (
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            request.mutate();
          }}
        >
          <ProblemNotice error={request.error} />
          <TextField
            label={t("portalSignIn.email")}
            type="email"
            autoComplete="email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <Button type="submit" variant="primary" size="lg" loading={request.isPending}>
            {t("portalReset.submit")}
          </Button>
        </form>
      )}
    </PublicLayout>
  );
}

/** The link in Portal Invitation, reset and Self-Registration emails. */
export function PortalSetPasswordPage({ token }: { token?: string | undefined }) {
  const t = useText();
  const set = useMutation({
    mutationFn: (password: string) =>
      call(api.POST("/api/v1/portal/password", { body: { token: token ?? "", password } })),
  });
  const invalid = Boolean(set.error) && problemOf(set.error).status !== 422;
  return (
    <PublicLayout area="portal" title={t("portalPassword.title")} footer={<BackToSignIn />}>
      {!token ? (
        <Notice tone="info">{t("portalPassword.noToken")}</Notice>
      ) : set.isSuccess ? (
        <Notice tone="success" title={t("portalPassword.done")}>
          <Link to="/portal/sign-in">{t("portalSignIn.submit")}</Link>
        </Notice>
      ) : (
        <>
          {invalid ? (
            <Notice tone="danger">
              {t("portalPassword.invalid")}{" "}
              <Link to="/portal/reset-password">{t("portalReset.submit")}</Link>
            </Notice>
          ) : (
            <ProblemNotice error={set.error} />
          )}
          <NewPasswordForm
            busy={set.isPending}
            submitLabel={t("portalPassword.submit")}
            onSubmit={(p) => set.mutate(p)}
          />
        </>
      )}
    </PublicLayout>
  );
}

/** Self-Registration with a work email address (FR-PRT-001), when it is switched on. */
export function PortalRegisterPage() {
  const t = useText();
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const options = useQuery({
    queryKey: ["portal", "sign-in-options"],
    queryFn: () => call(api.GET("/api/v1/portal/sign-in-options")),
  });
  const register = useMutation({
    mutationFn: () =>
      call(
        api.POST("/api/v1/portal/registration", {
          body: { full_name: name.trim(), email: email.trim() },
        }),
      ),
  });
  const problem = register.error ? problemOf(register.error) : null;
  return (
    <PublicLayout
      area="portal"
      title={t("portalRegister.title")}
      intro={t("portalRegister.intro")}
      footer={<BackToSignIn />}
    >
      {options.data && !options.data.self_registration ? (
        <Notice tone="info">{t("portalRegister.off")}</Notice>
      ) : register.isSuccess ? (
        <Notice tone="success">{t("portalRegister.sent")}</Notice>
      ) : (
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            register.mutate();
          }}
        >
          {problem?.status === 422 ? (
            <Notice tone="danger">{problem.detail ?? t("portalRegister.domain")}</Notice>
          ) : (
            <ProblemNotice error={register.error} />
          )}
          <TextField
            label={t("portalRegister.name")}
            autoComplete="name"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <TextField
            label={t("portalRegister.email")}
            type="email"
            autoComplete="email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <Button type="submit" variant="primary" size="lg" loading={register.isPending}>
            {t("portalRegister.submit")}
          </Button>
        </form>
      )}
    </PublicLayout>
  );
}
