import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { portalSignIn } from "@/features/portal/actions";
import { portalSessionQuery, portalSignInOptionsQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Problem } from "@/lib/problem";
import { safeRedirect } from "../auth/safeRedirect";
import { PublicShell } from "../PublicShell";

/**
 * Requester sign-in to the Self-Service Portal with a Portal Account (FR-PRT-001). Links
 * to the set-password email and, when it is on, Self-Registration.
 */
export function PortalSignInPage({ redirect }: { redirect: string | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const options = useQuery(portalSignInOptionsQuery);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [problem, setProblem] = useState<Problem | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setProblem(null);
    const result = await portalSignIn({ email, password });
    setBusy(false);
    if (result.ok) {
      queryClient.setQueryData(portalSessionQuery.queryKey, result.data);
      await navigate({ to: safeRedirect(redirect, "portal") });
      return;
    }
    setProblem(result.problem);
  }

  const message = (() => {
    switch (problem?.kind) {
      case undefined:
        return null;
      case "invalid-credentials":
        return t("portalSignIn.invalid");
      case "too-many-sign-in-attempts":
        return t("signIn.banned", { minutes: Math.ceil((problem.retryAfter ?? 60) / 60) });
      default:
        return t("common.error");
    }
  })();

  return (
    <PublicShell title={t("portalSignIn.title")} area={t("portal.title")}>
      <form className="grid gap-4" onSubmit={submit}>
        {message ? <Alert variant="error">{message}</Alert> : null}
        <Field
          label={t("portal.email")}
          name="email"
          type="email"
          autoComplete="username"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
        />
        <Field
          label={t("signIn.password")}
          name="password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Button type="submit" disabled={busy}>
          {busy ? t("signIn.submitting") : t("signIn.submit")}
        </Button>
        <div className="flex flex-col gap-1 text-sm">
          <Link
            to="/portal/reset-password"
            className="text-primary underline-offset-4 hover:underline"
          >
            {t("portalSignIn.forgot")}
          </Link>
          {options.data?.self_registration ? (
            <Link to="/portal/register" className="text-primary underline-offset-4 hover:underline">
              {t("portalSignIn.register")}
            </Link>
          ) : null}
          <Link to="/sign-in" className="text-muted-foreground underline-offset-4 hover:underline">
            {t("portalSignIn.analyst")}
          </Link>
        </div>
      </form>
    </PublicShell>
  );
}
