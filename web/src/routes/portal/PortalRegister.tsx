import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { registerForPortal } from "@/features/portal/actions";
import { useText } from "@/i18n/useText";
import { PublicShell } from "../PublicShell";
import { BackToPortalSignIn } from "./PortalLinks";

/**
 * Self-Registration (FR-PRT-001): name and work email; the emailed link confirms the
 * address and sets the password. The server decides whether it is on and which
 * addresses qualify (a Company's Sender Domain).
 */
export function PortalRegisterPage() {
  const t = useText();
  const [fullName, setFullName] = useState("");
  const [email, setEmail] = useState("");
  const [state, setState] = useState<"idle" | "busy" | "sent" | "off" | "domain" | "error">("idle");

  async function submit(event: FormEvent) {
    event.preventDefault();
    setState("busy");
    const result = await registerForPortal({ full_name: fullName, email });
    if (result.ok) {
      setState("sent");
    } else if (result.problem.status === 409) {
      setState("off");
    } else {
      setState(result.problem.status === 422 ? "domain" : "error");
    }
  }

  return (
    <PublicShell title={t("portalRegister.title")} area={t("portal.title")}>
      {state === "sent" ? (
        <div className="grid gap-4">
          <Alert variant="success">{t("portalRegister.sent")}</Alert>
          <BackToPortalSignIn />
        </div>
      ) : (
        <form className="grid gap-4" onSubmit={submit}>
          <p className="text-sm text-muted-foreground">{t("portalRegister.intro")}</p>
          {state === "off" ? <Alert variant="error">{t("portalRegister.off")}</Alert> : null}
          {state === "error" ? <Alert variant="error">{t("common.error")}</Alert> : null}
          <Field
            label={t("portalRegister.name")}
            name="name"
            autoComplete="name"
            required
            value={fullName}
            onChange={(e) => setFullName(e.target.value)}
          />
          <Field
            label={t("portalRegister.email")}
            name="email"
            type="email"
            autoComplete="email"
            required
            error={state === "domain" ? t("portalRegister.domain") : undefined}
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <Button type="submit" disabled={state === "busy"}>
            {t("portalRegister.submit")}
          </Button>
          <BackToPortalSignIn />
        </form>
      )}
    </PublicShell>
  );
}
