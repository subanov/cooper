import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useEffect, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { beginTotpEnrollment, confirmTotpEnrollment } from "@/features/session/actions";
import { analystSessionQuery } from "@/features/session/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { PublicShell } from "../PublicShell";

/** Two-factor enrollment (FR-IAM-003), required before anything else when policy says so. */
export function TwoFactorSetupPage() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const session = useQuery(analystSessionQuery);
  const [enrollment, setEnrollment] = useState<Schemas["TotpEnrollmentDto"] | null>(null);
  const [failed, setFailed] = useState(false);
  const [code, setCode] = useState("");
  const [wrong, setWrong] = useState(false);

  useEffect(() => {
    let active = true;
    beginTotpEnrollment().then((result) => {
      if (!active) {
        return;
      }
      if (result.ok) {
        setEnrollment(result.data);
      } else {
        setFailed(true);
      }
    });
    return () => {
      active = false;
    };
  }, []);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const result = await confirmTotpEnrollment(code.replace(/\s/g, ""));
    if (!result.ok) {
      setWrong(true);
      return;
    }
    await queryClient.invalidateQueries({ queryKey: analystSessionQuery.queryKey });
    await navigate({ to: "/" });
  }

  return (
    <PublicShell title={t("totp.title")}>
      <div className="grid gap-4">
        {session.data?.totp_enrollment_required ? (
          <Alert variant="info">{t("totp.required")}</Alert>
        ) : null}
        {failed ? <Alert variant="error">{t("common.error")}</Alert> : null}
        {enrollment ? (
          <form className="grid gap-4" onSubmit={submit}>
            <p className="text-sm">{t("totp.step1")}</p>
            <div className="grid gap-1">
              <span className="text-sm font-medium">{t("totp.secret")}</span>
              <code className="break-all rounded-md bg-muted px-3 py-2 font-mono text-sm tracking-wider">
                {enrollment.secret.replace(/(.{4})/g, "$1 ").trim()}
              </code>
              <a
                href={enrollment.otpauth_uri}
                className="text-sm text-primary underline-offset-4 hover:underline"
              >
                {t("totp.openApp")}
              </a>
            </div>
            <p className="text-sm">{t("totp.step2")}</p>
            <Field
              label={t("totp.code")}
              name="totp"
              inputMode="numeric"
              autoComplete="one-time-code"
              required
              error={wrong ? t("totp.wrong") : undefined}
              value={code}
              onChange={(e) => setCode(e.target.value)}
            />
            <Button type="submit">{t("totp.confirm")}</Button>
          </form>
        ) : failed ? null : (
          <p role="status">{t("common.loading")}</p>
        )}
      </div>
    </PublicShell>
  );
}
