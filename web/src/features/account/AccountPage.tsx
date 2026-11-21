import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { ShieldCheck } from "lucide-react";
import { useState } from "react";
import { Button } from "@/components/Button";
import { Card } from "@/components/Card";
import { Facts } from "@/components/Facts";
import { TextAreaField, TextField } from "@/components/Field";
import { Notice, ProblemNotice } from "@/components/Notice";
import { Page, PageHeader } from "@/components/Page";
import { useToast } from "@/components/Toast";
import { NewPasswordForm } from "@/features/auth/ResetPassword";
import { TotpEnrollment } from "@/features/auth/TwoFactorSetup";
import { analystSessionQuery, useSession } from "@/features/session/session";
import { useAction } from "@/features/shared/mutation";
import { useText } from "@/i18n/useText";
import { api, setCsrfToken } from "@/lib/api";
import { call } from "@/lib/request";

/** The signed-in Analyst's own settings: Signature, password and two-factor sign-in. */
export function AccountPage() {
  const t = useText();
  const session = useSession();
  const a = session.analyst;
  return (
    <Page width="medium">
      <PageHeader title={t("account.title")} description={t("account.intro")} />
      <Card title={t("account.profile")} description={t("account.profileHint")}>
        <Facts
          layout="inline"
          items={[
            { key: "name", term: t("common.name"), value: a.full_name },
            { key: "username", term: t("account.username"), value: a.username },
            { key: "email", term: t("account.email"), value: a.email },
          ]}
        />
      </Card>
      <SignatureCard initial={a.signature} />
      <PasswordCard />
      <TwoFactorCard enabled={a.totp_enabled} required={session.totp_enrollment_required} />
    </Page>
  );
}

function SignatureCard({ initial }: { initial: string }) {
  const t = useText();
  const [signature, setSignature] = useState(initial);
  const save = useAction(
    () => call(api.PUT("/api/v1/session/signature", { body: { signature } })),
    {
      invalidate: [analystSessionQuery.queryKey],
      success: "account.signatureSaved",
    },
  );
  return (
    <Card title={t("account.signature")}>
      <form
        className="grid gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <TextAreaField
          label={t("account.signature")}
          labelHidden
          hint={t("account.signatureHint")}
          rows={4}
          value={signature}
          onChange={(e) => setSignature(e.target.value)}
        />
        <ProblemNotice error={save.error} />
        <div>
          <Button
            type="submit"
            variant="primary"
            loading={save.isPending}
            disabled={signature === initial}
          >
            {t("common.save")}
          </Button>
        </div>
      </form>
    </Card>
  );
}

function PasswordCard() {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [current, setCurrent] = useState("");
  const change = useMutation({
    mutationFn: (next: string) =>
      call(
        api.PUT("/api/v1/session/password", {
          body: { current_password: current, new_password: next },
        }),
      ),
    onSuccess: async () => {
      setCsrfToken("analyst", undefined);
      queryClient.clear();
      toast({ tone: "success", message: t("account.passwordChanged") });
      await navigate({ to: "/sign-in" });
    },
  });
  return (
    <Card title={t("account.password")} description={t("account.passwordIntro")}>
      <div className="grid max-w-md gap-4">
        <TextField
          label={t("account.currentPassword")}
          type="password"
          autoComplete="current-password"
          required
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
        />
        <ProblemNotice error={change.error} />
        <NewPasswordForm
          busy={change.isPending}
          submitLabel={t("account.changePassword")}
          onSubmit={(next) => change.mutate(next)}
        />
      </div>
    </Card>
  );
}

function TwoFactorCard({ enabled, required }: { enabled: boolean; required: boolean }) {
  const t = useText();
  const queryClient = useQueryClient();
  const [setup, setSetup] = useState(false);
  const [code, setCode] = useState("");
  const off = useAction(
    () => call(api.DELETE("/api/v1/session/totp", { body: { code: code.replace(/\s/g, "") } })),
    {
      invalidate: [analystSessionQuery.queryKey],
      success: "account.turnedOff",
      onSuccess: () => setCode(""),
    },
  );
  return (
    <Card title={t("account.twoFactor")}>
      <div className="grid gap-4">
        <Notice tone={enabled ? "success" : "warning"} live={false}>
          {enabled ? t("account.twoFactorOn") : t("account.twoFactorOff")}
        </Notice>
        {enabled ? (
          required ? (
            <p className="text-sm text-fg-muted">{t("account.twoFactorRequired")}</p>
          ) : (
            <form
              className="flex max-w-md flex-wrap items-end gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                off.mutate();
              }}
            >
              <TextField
                className="flex-1"
                label={t("account.turnOffCode")}
                inputMode="numeric"
                autoComplete="one-time-code"
                value={code}
                onChange={(e) => setCode(e.target.value)}
              />
              <Button
                type="submit"
                variant="danger"
                loading={off.isPending}
                disabled={!code.trim()}
              >
                {t("account.turnOff")}
              </Button>
              <ProblemNotice error={off.error} className="w-full" />
            </form>
          )
        ) : setup ? (
          <div className="max-w-md">
            <TotpEnrollment
              onDone={() => {
                setSetup(false);
                void queryClient.invalidateQueries({ queryKey: analystSessionQuery.queryKey });
              }}
            />
          </div>
        ) : (
          <div>
            <Button
              variant="primary"
              icon={<ShieldCheck aria-hidden="true" className="size-4" />}
              onClick={() => setSetup(true)}
            >
              {t("account.setUp")}
            </Button>
          </div>
        )}
      </div>
    </Card>
  );
}
