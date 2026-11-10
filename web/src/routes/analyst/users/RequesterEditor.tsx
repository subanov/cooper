import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field } from "@/components/ui/field";
import { SelectField } from "@/components/ui/select-field";
import { problemText } from "@/features/result";
import {
  createRequester,
  sendPortalInvitation,
  setRequesterActive,
  updateRequester,
} from "@/features/users/actions";
import { companiesQuery, defaultCompanyId, requesterQuery } from "@/features/users/queries";
import { useText } from "@/i18n/useText";
import type { Schemas } from "@/lib/api";
import { useFlash } from "./Flash";

type Requester = Schemas["RequesterDto"];

/** Creates a Requester (`id` absent) or edits one. */
export function RequesterEditor({ id }: { id?: string | undefined }) {
  const t = useText();
  const existing = useQuery({ ...requesterQuery(id ?? ""), enabled: Boolean(id) });
  if (id && existing.isPending) {
    return <p>{t("common.loading")}</p>;
  }
  if (id && !existing.data) {
    return <Alert variant="error">{t("common.error")}</Alert>;
  }
  return (
    <div className="grid max-w-2xl gap-6">
      <RequesterForm key={existing.data?.updated_at ?? "new"} requester={existing.data} />
      {existing.data ? <Lifecycle requester={existing.data} /> : null}
    </div>
  );
}

function RequesterForm({ requester }: { requester: Requester | undefined }) {
  const t = useText();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const flash = useFlash();
  const companies = useQuery(companiesQuery);
  const [fullName, setFullName] = useState(requester?.full_name ?? "");
  const [email, setEmail] = useState(requester?.email ?? "");
  const [phone, setPhone] = useState(requester?.phone ?? "");
  const [username, setUsername] = useState(requester?.username ?? "");
  const [companyId, setCompanyId] = useState("");
  const company = companyId || defaultCompanyId(companies.data);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const body: Schemas["RequesterRequest"] = {
      full_name: fullName.trim(),
      email: email.trim(),
      phone: phone.trim() || null,
      username: username.trim() || null,
      ...(!requester && company ? { company_id: company } : {}),
    };
    const outcome = requester
      ? await updateRequester(requester.id, body)
      : await createRequester(body);
    if (!outcome.ok) {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: ["users", "requesters"] });
    if (requester) {
      flash.ok(t("users.saved"));
    } else {
      await navigate({ to: "/users/requesters/$id", params: { id: outcome.data.id } });
    }
  }

  return (
    <form onSubmit={submit} noValidate className="grid gap-3" aria-labelledby="requester-details">
      <h2 id="requester-details" className="text-lg font-semibold">
        {requester ? requester.full_name : t("users.newRequester")}
        {requester && !requester.is_active ? (
          <Badge tone="muted" className="ml-2">
            {t("users.deactivated")}
          </Badge>
        ) : null}
      </h2>
      <Field
        label={t("users.fullName")}
        value={fullName}
        onChange={(e) => setFullName(e.target.value)}
        required
      />
      <Field
        label={t("users.email")}
        type="email"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        required
      />
      <Field
        label={t("users.phone")}
        type="tel"
        value={phone}
        onChange={(e) => setPhone(e.target.value)}
      />
      <Field
        label={t("users.username")}
        value={username}
        onChange={(e) => setUsername(e.target.value)}
      />
      {requester ? null : (
        <SelectField
          label={t("users.company")}
          value={company}
          onChange={(e) => setCompanyId(e.target.value)}
        >
          {(companies.data ?? []).map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
      )}
      <div className="flex items-center gap-3">
        <Button type="submit">{requester ? t("users.save") : t("users.create")}</Button>
        {flash.element}
      </div>
    </form>
  );
}

function Lifecycle({ requester }: { requester: Requester }) {
  const t = useText();
  const queryClient = useQueryClient();
  const flash = useFlash();

  async function toggle() {
    const outcome = await setRequesterActive(requester.id, !requester.is_active);
    if (outcome.ok) {
      await queryClient.invalidateQueries({ queryKey: ["users", "requesters"] });
      flash.ok(t(requester.is_active ? "users.deactivatedDone" : "users.reactivatedDone"));
    } else {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
    }
  }
  async function invite() {
    const outcome = await sendPortalInvitation(requester.id);
    if (outcome.ok) {
      flash.ok(t("users.invitationSent", { email: requester.email }));
    } else {
      flash.fail(t("users.failed", { detail: problemText(outcome.problem) }));
    }
  }

  return (
    <section aria-labelledby="requester-lifecycle" className="grid gap-3">
      <h2 id="requester-lifecycle" className="text-lg font-semibold">
        {t("users.account")}
      </h2>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          onClick={() => void invite()}
          disabled={!requester.is_active}
        >
          {t("users.sendInvitation")}
        </Button>
        <Button
          size="sm"
          variant={requester.is_active ? "destructive" : "outline"}
          onClick={() => void toggle()}
        >
          {t(requester.is_active ? "users.deactivate" : "users.reactivate")}
        </Button>
      </div>
      {flash.element}
    </section>
  );
}
