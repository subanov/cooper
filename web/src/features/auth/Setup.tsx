import { useMutation, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { Button } from "@/components/Button";
import { LoadingBlock, Notice, ProblemNotice } from "@/components/Feedback";
import { Fieldset, TextField } from "@/components/Field";
import { fieldError } from "@/features/shared/mutation";
import { PublicLayout } from "@/features/shell/PublicLayout";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { call } from "@/lib/request";

/** First-run setup: the default Company and the first Administrator (FR-SYS-004). */
export function SetupPage() {
  const t = useText();
  const status = useQuery({ queryKey: ["setup"], queryFn: () => call(api.GET("/api/v1/setup")) });
  const [form, setForm] = useState({
    company_name: "",
    ticket_code: "",
    sender_domains: "",
    full_name: "",
    username: "",
    email: "",
    password: "",
  });
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));
  const setup = useMutation({
    mutationFn: () =>
      call(
        api.POST("/api/v1/setup", {
          body: {
            ...form,
            company_name: form.company_name.trim(),
            ticket_code: form.ticket_code.trim().toUpperCase(),
            sender_domains: form.sender_domains
              .split(/[,\s]+/)
              .map((d) => d.trim())
              .filter(Boolean),
          },
        }),
      ),
  });
  const err = (field: string) => fieldError(setup.error, field);
  return (
    <PublicLayout title={t("setup.title")} intro={t("setup.intro")}>
      {status.isPending ? (
        <LoadingBlock />
      ) : setup.isSuccess ? (
        <Notice tone="success" title={t("setup.done")}>
          <Link to="/sign-in" className="font-medium text-accent-text underline">
            {t("signIn.submit")}
          </Link>
        </Notice>
      ) : status.data && !status.data.required ? (
        <Notice tone="info" title={t("setup.notNeeded")}>
          <Link to="/sign-in" className="font-medium text-accent-text underline">
            {t("signIn.submit")}
          </Link>
        </Notice>
      ) : (
        <form
          className="grid gap-6"
          onSubmit={(e) => {
            e.preventDefault();
            setup.mutate();
          }}
        >
          <ProblemNotice error={setup.error} />
          <Fieldset legend={t("setup.company")}>
            <TextField
              label={t("setup.companyName")}
              required
              value={form.company_name}
              onChange={set("company_name")}
              error={err("company_name")}
            />
            <TextField
              label={t("setup.ticketCode")}
              hint={t("setup.ticketCodeHint")}
              required
              maxLength={5}
              value={form.ticket_code}
              onChange={set("ticket_code")}
              error={err("ticket_code")}
            />
            <TextField
              label={t("setup.senderDomains")}
              hint={t("setup.senderDomainsHint")}
              value={form.sender_domains}
              onChange={set("sender_domains")}
            />
          </Fieldset>
          <Fieldset legend={t("setup.administrator")}>
            <TextField
              label={t("setup.fullName")}
              autoComplete="name"
              required
              value={form.full_name}
              onChange={set("full_name")}
              error={err("full_name")}
            />
            <TextField
              label={t("setup.username")}
              autoComplete="username"
              required
              value={form.username}
              onChange={set("username")}
              error={err("username")}
            />
            <TextField
              label={t("setup.email")}
              type="email"
              autoComplete="email"
              required
              value={form.email}
              onChange={set("email")}
              error={err("email")}
            />
            <TextField
              label={t("setup.password")}
              hint={t("reset.passwordHint")}
              type="password"
              autoComplete="new-password"
              required
              value={form.password}
              onChange={set("password")}
              error={err("password")}
            />
          </Fieldset>
          <Button type="submit" variant="primary" size="lg" loading={setup.isPending}>
            {t("setup.submit")}
          </Button>
        </form>
      )}
    </PublicLayout>
  );
}
