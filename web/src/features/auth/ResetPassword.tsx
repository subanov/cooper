import { useMutation } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Button } from "@/components/Button";
import { Notice, ProblemNotice } from "@/components/Feedback";
import { TextField } from "@/components/Field";
import { PublicLayout } from "@/features/shell/PublicLayout";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { problemOf } from "@/lib/problem";
import { call } from "@/lib/request";

/**
 * Asks for a reset link, or (with the emailed `token`) sets a new password (FR-IAM-004).
 * The answer never says whether an account exists.
 */
export function ResetPasswordPage({ token }: { token: string | undefined }) {
  return token ? <ChooseNewPassword token={token} /> : <RequestLink />;
}

function RequestLink() {
  const t = useText();
  const [login, setLogin] = useState("");
  const request = useMutation({
    mutationFn: () => call(api.POST("/api/v1/password-reset", { body: { login: login.trim() } })),
  });
  return (
    <PublicLayout title={t("reset.title")} intro={t("reset.intro")} footer={<BackToSignIn />}>
      {request.isSuccess ? (
        <Notice tone="success">{t("reset.sent")}</Notice>
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
            label={t("signIn.login")}
            autoComplete="username"
            required
            autoFocus
            value={login}
            onChange={(e) => setLogin(e.target.value)}
          />
          <Button type="submit" variant="primary" size="lg" loading={request.isPending}>
            {t("reset.submit")}
          </Button>
        </form>
      )}
    </PublicLayout>
  );
}

/** A new password, typed twice. Length is the only client-side check; the API decides. */
export function NewPasswordForm({
  onSubmit,
  busy,
  submitLabel,
}: {
  onSubmit: (password: string) => void;
  busy: boolean;
  submitLabel: string;
}) {
  const t = useText();
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [shown, setShown] = useState(false);
  const short = password.length < 12;
  const mismatch = repeat !== password;
  function submit(event: FormEvent) {
    event.preventDefault();
    setShown(true);
    if (short || mismatch) {
      document.getElementById(short ? "new-password" : "repeat-password")?.focus();
      return;
    }
    onSubmit(password);
  }
  return (
    <form className="grid gap-4" onSubmit={submit} noValidate>
      <TextField
        id="new-password"
        label={t("reset.newPassword")}
        hint={t("reset.passwordHint")}
        type="password"
        autoComplete="new-password"
        required
        value={password}
        error={shown && short ? t("reset.tooShort") : undefined}
        onChange={(e) => setPassword(e.target.value)}
      />
      <TextField
        id="repeat-password"
        label={t("reset.confirmPassword")}
        type="password"
        autoComplete="new-password"
        required
        value={repeat}
        error={shown && !short && mismatch ? t("reset.mismatch") : undefined}
        onChange={(e) => setRepeat(e.target.value)}
      />
      <Button type="submit" variant="primary" size="lg" loading={busy}>
        {submitLabel}
      </Button>
    </form>
  );
}

function ChooseNewPassword({ token }: { token: string }) {
  const t = useText();
  const confirm = useMutation({
    mutationFn: (password: string) =>
      call(api.POST("/api/v1/password-reset/confirm", { body: { token, password } })),
  });
  // A bad or expired token is refused as a whole; a weak password is a 422 about the field.
  const invalid = Boolean(confirm.error) && problemOf(confirm.error).status !== 422;
  return (
    <PublicLayout title={t("reset.newTitle")} footer={<BackToSignIn />}>
      {confirm.isSuccess ? (
        <Notice tone="success" title={t("reset.done")}>
          <Link to="/sign-in" className="font-medium text-accent-text underline">
            {t("reset.back")}
          </Link>
        </Notice>
      ) : (
        <>
          {invalid ? (
            <Notice tone="danger">
              {t("reset.invalid")}{" "}
              <Link to="/reset-password" className="font-medium underline">
                {t("reset.submit")}
              </Link>
            </Notice>
          ) : (
            <ProblemNotice error={confirm.error} />
          )}
          <NewPasswordForm
            busy={confirm.isPending}
            submitLabel={t("reset.save")}
            onSubmit={(password) => confirm.mutate(password)}
          />
        </>
      )}
    </PublicLayout>
  );
}

function BackToSignIn() {
  const t = useText();
  return (
    <Link to="/sign-in" className="text-accent-text hover:underline">
      {t("reset.back")}
    </Link>
  );
}
