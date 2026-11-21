import * as RadixDialog from "@radix-ui/react-dialog";
import { useNavigate } from "@tanstack/react-router";
import { Command } from "cmdk";
import {
  Keyboard,
  LogOut,
  Monitor,
  Moon,
  Plus,
  Rows3,
  Search,
  Sun,
  TicketCheck,
} from "lucide-react";
import { type ReactNode, useState } from "react";
import { Kbd } from "@/components/Facts";
import { useToast } from "@/components/Toast";
import { useCan, useModuleOn, useSession, useSignOut } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { api } from "@/lib/api";
import { setPreferences, usePreferences } from "@/lib/preferences";
import { visibleNav } from "./nav";

const itemClass =
  "flex h-9 cursor-pointer select-none items-center gap-2.5 rounded-md px-2.5 text-sm data-[selected=true]:bg-accent-soft data-[selected=true]:text-fg [&_svg]:size-4 [&_svg]:text-fg-muted";

/**
 * The command palette (Ctrl/⌘+K): every screen and common action by name, and a Ticket
 * Number to open it straight away (Jakob's Law: as in Linear or GitHub).
 */
export function CommandPalette({
  open,
  onOpenChange,
  onShowShortcuts,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onShowShortcuts: () => void;
}) {
  const t = useText();
  const navigate = useNavigate();
  const session = useSession();
  const can = useCan();
  const moduleOn = useModuleOn();
  const signOut = useSignOut();
  const toast = useToast();
  const prefs = usePreferences();
  const [query, setQuery] = useState("");
  const items = visibleNav(session, moduleOn);
  const trimmed = query.trim();

  function run(action: () => void | Promise<void>) {
    onOpenChange(false);
    setQuery("");
    void action();
  }

  async function openNumber(number: string) {
    const { data } = await api.GET("/api/v1/tickets/by-number/{number}", {
      params: { path: { number } },
    });
    if (data) {
      await navigate({ to: "/tickets/$ticketId", params: { ticketId: data.id } });
    } else {
      toast({ tone: "danger", message: t("problem.notFound") });
    }
  }

  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="fixed inset-0 z-40 bg-overlay" />
        <RadixDialog.Content
          aria-describedby={undefined}
          className="fixed left-1/2 top-[12vh] z-50 w-[calc(100vw-2rem)] max-w-xl -translate-x-1/2 overflow-hidden rounded-lg border border-line bg-surface shadow-overlay"
        >
          <RadixDialog.Title className="sr-only">{t("palette.title")}</RadixDialog.Title>
          <Command label={t("palette.title")} loop>
            <div className="flex items-center gap-2 border-b border-line px-3">
              <Search aria-hidden="true" className="size-4 text-fg-muted" />
              <Command.Input
                value={query}
                onValueChange={setQuery}
                placeholder={t("palette.placeholder")}
                className="h-12 w-full bg-transparent text-sm outline-none placeholder:text-fg-muted"
              />
            </div>
            <Command.List className="max-h-[60vh] overflow-y-auto p-2">
              <Command.Empty className="px-3 py-6 text-center text-sm text-fg-muted">
                {t("palette.empty")}
              </Command.Empty>
              {trimmed && can("tickets.read") ? (
                <Command.Group heading={t("nav.tickets")} className={groupClass}>
                  {/\d/.test(trimmed) ? (
                    <Command.Item
                      value={`open-ticket ${trimmed}`}
                      onSelect={() => run(() => openNumber(trimmed.toUpperCase()))}
                      className={itemClass}
                    >
                      <TicketCheck aria-hidden="true" />
                      {t("palette.openTicket", { number: trimmed.toUpperCase() })}
                    </Command.Item>
                  ) : null}
                  <Command.Item
                    value={`search-tickets ${trimmed}`}
                    onSelect={() =>
                      run(() => navigate({ to: "/tickets", search: { view: "open", q: trimmed } }))
                    }
                    className={itemClass}
                  >
                    <Search aria-hidden="true" />
                    {t("palette.searchTickets", { query: trimmed })}
                  </Command.Item>
                </Command.Group>
              ) : null}
              <Command.Group heading={t("palette.goTo")} className={groupClass}>
                {items.map((item) => (
                  <Command.Item
                    key={String(item.to)}
                    value={t(item.label)}
                    onSelect={() => run(() => navigate({ to: item.to }))}
                    className={itemClass}
                  >
                    <item.icon aria-hidden="true" />
                    <span className="flex-1">{t(item.label)}</span>
                    <Shortcut keys={["g", item.shortcut]} />
                  </Command.Item>
                ))}
                <Command.Item
                  value={t("nav.notifications")}
                  onSelect={() => run(() => navigate({ to: "/notifications" }))}
                  className={itemClass}
                >
                  <span className="flex-1 pl-[1.625rem]">{t("nav.notifications")}</span>
                </Command.Item>
                <Command.Item
                  value={t("nav.account")}
                  onSelect={() => run(() => navigate({ to: "/account" }))}
                  className={itemClass}
                >
                  <span className="flex-1 pl-[1.625rem]">{t("nav.account")}</span>
                </Command.Item>
              </Command.Group>
              <Command.Group heading={t("palette.actions")} className={groupClass}>
                {can("tickets.create") ? (
                  <PaletteItem
                    icon={<Plus />}
                    label={t("shell.newTicket")}
                    keys={["c"]}
                    onSelect={() =>
                      run(() => navigate({ to: "/tickets/new", search: { view: "open" } }))
                    }
                  />
                ) : null}
                {prefs.theme !== "light" ? (
                  <PaletteItem
                    icon={<Sun />}
                    label={t("palette.themeLight")}
                    onSelect={() => run(() => setPreferences({ theme: "light" }))}
                  />
                ) : null}
                {prefs.theme !== "dark" ? (
                  <PaletteItem
                    icon={<Moon />}
                    label={t("palette.themeDark")}
                    onSelect={() => run(() => setPreferences({ theme: "dark" }))}
                  />
                ) : null}
                {prefs.theme !== "system" ? (
                  <PaletteItem
                    icon={<Monitor />}
                    label={t("palette.themeSystem")}
                    onSelect={() => run(() => setPreferences({ theme: "system" }))}
                  />
                ) : null}
                <PaletteItem
                  icon={<Rows3 />}
                  label={t("palette.density")}
                  onSelect={() =>
                    run(() =>
                      setPreferences({
                        density: prefs.density === "compact" ? "comfortable" : "compact",
                      }),
                    )
                  }
                />
                <PaletteItem
                  icon={<Keyboard />}
                  label={t("palette.shortcuts")}
                  keys={["?"]}
                  onSelect={() => run(onShowShortcuts)}
                />
                <PaletteItem
                  icon={<LogOut />}
                  label={t("palette.signOut")}
                  onSelect={() => run(signOut)}
                />
              </Command.Group>
            </Command.List>
          </Command>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

const groupClass =
  "[&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-fg-muted";

function PaletteItem({
  icon,
  label,
  keys,
  onSelect,
}: {
  icon: ReactNode;
  label: string;
  keys?: string[];
  onSelect: () => void;
}) {
  return (
    <Command.Item value={label} onSelect={onSelect} className={itemClass}>
      <span aria-hidden="true" className="contents">
        {icon}
      </span>
      <span className="flex-1">{label}</span>
      {keys ? <Shortcut keys={keys} /> : null}
    </Command.Item>
  );
}

function Shortcut({ keys }: { keys: string[] }) {
  return (
    <span aria-hidden="true" className="flex gap-1">
      {keys.map((k) => (
        <Kbd key={k}>{k}</Kbd>
      ))}
    </span>
  );
}
