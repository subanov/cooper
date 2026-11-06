import { Link } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { confirmPasswordReset, requestPasswordReset } from "@/features/session/actions";
import { useText } from "@/i18n/useText";
import { PublicShell } from "../PublicShell";

/**
 * Password reset (FR-IAM-004). Without a token: ask for the link. With the token from
 * the email (`/reset-password?token=…`): choose a new password.
 */
export function ResetPasswordPage({ token }: { token: string | undefined }) {
  return token ? <ChooseNewPassword token={token} /> : <RequestLink />;
}

function BackToSignIn() {
  const t = useText();
  return (
    <Link to="/sign-in" className="text-sm text-primary underline-offset-4 hover:underline">
      {t("reset.back")}
    </Link>
  );
}

function RequestLink() {
  const t = useText();
  const [login, setLogin] = useState("");
  const [state, setState] = useState<"idle" | "busy" | "sent" | "error">("idle");

  async function submit(event: FormEvent) {
    event.preventDefault();
    setState("busy");
    const result = await requestPasswordReset(login);
    setState(result.ok ? "sent" : "error");
  }

  return (
    <PublicShell title={t("reset.title")}>
      {state === "sent" ? (
        <div className="grid gap-4">
          <Alert variant="success">{t("reset.sent")}</Alert>
          <BackToSignIn />
        </div>
      ) : (
        <form className="grid gap-4" onSubmit={submit}>
          <p className="text-sm text-muted-foreground">{t("reset.intro")}</p>
          {state === "error" ? <Alert variant="error">{t("common.error")}</Alert> : null}
          <Field
            label={t("signIn.login")}
            name="username"
            autoComplete="username"
            required
            value={login}
            onChange={(e) => setLogin(e.target.value)}
          />
          <Button type="submit" disabled={state === "busy"}>
            {t("reset.submit")}
          </Button>
          <BackToSignIn />
        </form>
      )}
    </PublicShell>
  );
}

function ChooseNewPassword({ token }: { token: string }) {
  const t = useText();
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [mismatch, setMismatch] = useState(false);
  const [state, setState] = useState<"idle" | "busy" | "done" | "invalid" | "error">("idle");

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (password !== repeat) {
      setMismatch(true);
      return;
    }
    setMismatch(false);
    setState("busy");
    const result = await confirmPasswordReset(token, password);
    if (result.ok) {
      setState("done");
    } else {
      setState(result.problem.status === 401 ? "invalid" : "error");
    }
  }

  return (
    <PublicShell title={t("reset.newTitle")}>
      {state === "done" ? (
        <div className="grid gap-4">
          <Alert variant="success">{t("reset.done")}</Alert>
          <BackToSignIn />
        </div>
      ) : (
        <form className="grid gap-4" onSubmit={submit}>
          {state === "invalid" ? <Alert variant="error">{t("reset.invalid")}</Alert> : null}
          {state === "error" ? <Alert variant="error">{t("common.error")}</Alert> : null}
          <Field
            label={t("reset.newPassword")}
            hint={t("reset.passwordHint")}
            type="password"
            name="new-password"
            autoComplete="new-password"
            minLength={12}
            required
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <Field
            label={t("reset.confirmPassword")}
            type="password"
            name="confirm-password"
            autoComplete="new-password"
            required
            error={mismatch ? t("reset.mismatch") : undefined}
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
          />
          <Button type="submit" disabled={state === "busy"}>
            {t("reset.save")}
          </Button>
          <BackToSignIn />
        </form>
      )}
    </PublicShell>
  );
}
