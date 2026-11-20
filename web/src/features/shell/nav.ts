import type { LinkProps } from "@tanstack/react-router";
import {
  Activity,
  BookOpen,
  Boxes,
  CalendarDays,
  ClipboardCheck,
  FileText,
  House,
  type LucideIcon,
  Settings,
  SquareKanban,
  Ticket,
  Users,
} from "lucide-react";
import type { Session } from "@/features/session/session";
import { can } from "@/features/session/session";
import type { MessageId } from "@/i18n/messages";

/** One entry of the Analyst navigation and the command palette. */
export interface NavItem {
  to: LinkProps["to"];
  label: MessageId;
  icon: LucideIcon;
  group: "work" | "knowledge" | "records" | "manage";
  /** The Module that must be on (FR-GEN-005). */
  module?: string;
  /** Any one of these Capabilities shows the entry. */
  capabilities?: readonly string[];
  /** Second key of the `g` shortcut (`g t`). */
  shortcut: string;
}

/** Capabilities that open at least one Administration page. */
export const ADMIN_CAPABILITIES = [
  "system.manage",
  "system.view_log",
  "tickets.configure",
  "companies.manage",
  "teams.manage",
  "departments.manage",
  "roles.manage",
  "assets.configure",
  "contracts.configure",
  "calendar.configure",
] as const;

/**
 * The Analyst navigation, most-used first and Administration last (Serial Position
 * Effect). Hiding entries tidies the screen; the API still checks every request.
 */
export const NAV_ITEMS: readonly NavItem[] = [
  { to: "/", label: "nav.home", icon: House, group: "work", shortcut: "h" },
  {
    to: "/tickets",
    label: "nav.tickets",
    icon: Ticket,
    group: "work",
    module: "tickets",
    capabilities: ["tickets.read"],
    shortcut: "t",
  },
  {
    to: "/tasks",
    label: "nav.tasks",
    icon: SquareKanban,
    group: "work",
    module: "tasks",
    capabilities: ["tasks.read"],
    shortcut: "w",
  },
  {
    to: "/morning-checks",
    label: "nav.morningChecks",
    icon: ClipboardCheck,
    group: "work",
    module: "morning-checks",
    capabilities: ["morning-checks.read"],
    shortcut: "m",
  },
  {
    to: "/calendar",
    label: "nav.calendar",
    icon: CalendarDays,
    group: "work",
    module: "calendar",
    capabilities: ["calendar.read"],
    shortcut: "l",
  },
  {
    to: "/knowledge",
    label: "nav.knowledge",
    icon: BookOpen,
    group: "knowledge",
    module: "knowledge",
    capabilities: ["knowledge.read"],
    shortcut: "k",
  },
  {
    to: "/service-status",
    label: "nav.serviceStatus",
    icon: Activity,
    group: "knowledge",
    module: "service-status",
    capabilities: ["service-status.read"],
    shortcut: "s",
  },
  {
    to: "/assets",
    label: "nav.assets",
    icon: Boxes,
    group: "records",
    module: "assets",
    capabilities: ["assets.read"],
    shortcut: "i",
  },
  {
    to: "/contracts",
    label: "nav.contracts",
    icon: FileText,
    group: "records",
    module: "contracts",
    capabilities: ["contracts.read"],
    shortcut: "c",
  },
  {
    to: "/users",
    label: "nav.users",
    icon: Users,
    group: "manage",
    capabilities: ["analysts.manage", "requesters.read"],
    shortcut: "u",
  },
  {
    to: "/administration",
    label: "nav.administration",
    icon: Settings,
    group: "manage",
    capabilities: ADMIN_CAPABILITIES,
    shortcut: "a",
  },
];

export const NAV_GROUPS: readonly { id: NavItem["group"]; label: MessageId }[] = [
  { id: "work", label: "nav.group.work" },
  { id: "knowledge", label: "nav.group.knowledge" },
  { id: "records", label: "nav.group.records" },
  { id: "manage", label: "nav.group.manage" },
];

/** The entries this Analyst sees: Module on and a Capability held. */
export function visibleNav(session: Session, moduleOn: (key: string) => boolean): NavItem[] {
  return NAV_ITEMS.filter(
    (item) =>
      (!item.module || moduleOn(item.module)) &&
      (!item.capabilities || item.capabilities.some((c) => can(session, c))),
  );
}
