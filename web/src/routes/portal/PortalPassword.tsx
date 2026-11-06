import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { requestPortalReset, setPortalPassword } from "@/features/portal/actions";
import { useText } from "@/i18n/useText";
import { PublicShell } from "../PublicShell";
import { BackToPortalSignIn } from "./PortalLinks";

/**
 * Asks for a link to set a Portal password: a forgotten password, or a first one for a
 * Requester the service desk already knows (FR-PRT-001). The answer never says whether
 * the address is known.
 */
export function PortalResetPage() {
  const t = useText();
  const [email, setEmail] = useState("");
  const [state, setState] = useState<"idle" | "busy" | "sent" | "error">("idle");

  async function submit(event: FormEvent) {
    event.preventDefault();
    setState("busy");
    const result = await requestPortalReset(email);
    setState(result.ok ? "sent" : "error");
  }

  return (
    <PublicShell title={t("portalReset.title")} area={t("portal.title")}>
      {state === "sent" ? (
        <div className="grid gap-4">
          <Alert variant="success">{t("portalReset.sent")}</Alert>
          <BackToPortalSignIn />
        </div>
      ) : (
        <form className="grid gap-4" onSubmit={submit}>
          <p className="text-sm text-muted-foreground">{t("portalReset.intro")}</p>
          {state === "error" ? <Alert variant="error">{t("common.error")}</Alert> : null}
          <Field
            label={t("portal.email")}
            name="email"
            type="email"
            autoComplete="username"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <Button type="submit" disabled={state === "busy"}>
            {t("reset.submit")}
          </Button>
          <BackToPortalSignIn />
        </form>
      )}
    </PublicShell>
  );
}

/**
 * Sets the Portal password with the token from a Portal Invitation, reset or
 * Self-Registration email (`/portal/set-password?token=…`).
 */
export function PortalSetPasswordPage({ token }: { token: string | undefined }) {
  const t = useText();
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [mismatch, setMismatch] = useState(false);
  const [state, setState] = useState<"idle" | "busy" | "done" | "invalid" | "short" | "error">(
    token ? "idle" : "invalid",
  );

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (password !== repeat) {
      setMismatch(true);
      return;
    }
    setMismatch(false);
    if (!token) {
      setState("invalid");
      return;
    }
    setState("busy");
    const result = await setPortalPassword(token, password);
    if (result.ok) {
      setState("done");
    } else if (result.problem.status === 401) {
      setState("invalid");
    } else {
      setState(result.problem.status === 422 ? "short" : "error");
    }
  }

  return (
    <PublicShell title={t("portalPassword.title")} area={t("portal.title")}>
      {state === "done" ? (
        <div className="grid gap-4">
          <Alert variant="success">{t("portalPassword.done")}</Alert>
          <BackToPortalSignIn />
        </div>
      ) : (
        <form className="grid gap-4" onSubmit={submit}>
          {state === "invalid" ? (
            <Alert variant="error">{t("portalPassword.invalid")}</Alert>
          ) : null}
          {state === "error" ? <Alert variant="error">{t("common.error")}</Alert> : null}
          <Field
            label={t("reset.newPassword")}
            hint={t("reset.passwordHint")}
            error={state === "short" ? t("portalPassword.short") : undefined}
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
          <BackToPortalSignIn />
        </form>
      )}
    </PublicShell>
  );
}
