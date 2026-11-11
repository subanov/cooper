import type { MessageId } from "@/i18n/messages";
import type { Schemas } from "@/lib/api";

/** One entry of the Analyst navigation and command palette. */
export interface NavItem {
  /** Route path. */
  to:
    | "/"
    | "/tickets"
    | "/tasks"
    | "/knowledge"
    | "/service-status"
    | "/morning-checks"
    | "/contracts"
    | "/users"
    | "/administration";
  /** Catalogue id of the label. */
  label: MessageId;
  /** The Module that must be on, if any (FR-GEN-005). */
  module?: string;
  /** The Capability the Analyst needs, if any. */
  capability?: string;
  /** Second key of the `g` shortcut, for example `t` for `g t`. */
  shortcut: string;
}

/**
 * The Analyst navigation. Screens of later Modules add a line here. Hiding entries only
 * tidies the UI; the API still checks every request (ADR-0010).
 */
export const NAV_ITEMS: readonly NavItem[] = [
  { to: "/", label: "nav.home", shortcut: "h" },
  {
    to: "/tickets",
    label: "nav.tickets",
    module: "tickets",
    capability: "tickets.read",
    shortcut: "t",
  },
  { to: "/tasks", label: "nav.tasks", module: "tasks", capability: "tasks.read", shortcut: "w" },
  {
    to: "/knowledge",
    label: "nav.knowledge",
    module: "knowledge",
    capability: "knowledge.read",
    shortcut: "k",
  },
  {
    to: "/service-status",
    label: "nav.serviceStatus",
    module: "service-status",
    capability: "service-status.read",
    shortcut: "s",
  },
  {
    to: "/morning-checks",
    label: "nav.morningChecks",
    module: "morning-checks",
    capability: "morning-checks.read",
    shortcut: "m",
  },
  {
    to: "/contracts",
    label: "nav.contracts",
    module: "contracts",
    capability: "contracts.read",
    shortcut: "c",
  },
  {
    to: "/users",
    label: "nav.users",
    capability: "analysts.manage",
    shortcut: "u",
  },
  {
    to: "/administration",
    label: "nav.administration",
    capability: "system.manage",
    shortcut: "a",
  },
];

/** Whether the session holds a Capability (Administrators hold every one). */
export function can(session: Schemas["SessionDto"], capability: string): boolean {
  return session.is_admin || session.capabilities.includes(capability);
}

/** The entries this Analyst sees: Module on and Capability held. */
export function visibleNavItems(
  session: Schemas["SessionDto"],
  modules: readonly Schemas["ModuleDto"][] | undefined,
): NavItem[] {
  const enabled = new Set((modules ?? []).filter((m) => m.is_enabled).map((m) => m.key));
  return NAV_ITEMS.filter(
    (item) =>
      (!item.module || modules === undefined || enabled.has(item.module)) &&
      (!item.capability || can(session, item.capability)),
  );
}
