import type { LinkProps } from "@tanstack/react-router";
import type { MessageId } from "@/i18n/messages";

export interface AdminSection {
  to: LinkProps["to"];
  label: MessageId;
  group: "general" | "tickets" | "organisation" | "assets" | "contracts" | "calendar";
  /** Any one of these opens the page. The API checks every request. */
  capabilities: readonly string[];
  /** The Module that must be on. */
  module?: string;
}

/** Every Administration page, grouped as the side navigation shows them. */
export const ADMIN_SECTIONS: readonly AdminSection[] = [
  {
    to: "/administration/settings",
    label: "admin.settings",
    group: "general",
    capabilities: ["system.manage"],
  },
  {
    to: "/administration/modules",
    label: "admin.modules",
    group: "general",
    capabilities: ["system.manage"],
  },
  {
    to: "/administration/jobs",
    label: "admin.jobs",
    group: "general",
    capabilities: ["system.manage"],
  },
  {
    to: "/administration/log",
    label: "admin.log",
    group: "general",
    capabilities: ["system.view_log", "system.manage"],
  },
  {
    to: "/administration/ticket-statuses",
    label: "admin.ticketStatuses",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/ticket-priorities",
    label: "admin.ticketPriorities",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/ticket-types",
    label: "admin.ticketTypes",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/ticket-origins",
    label: "admin.ticketOrigins",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/resolution-codes",
    label: "admin.resolutionCodes",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/ticket-categories",
    label: "admin.ticketCategories",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/mailboxes",
    label: "admin.mailboxes",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/suggested-reply-instructions",
    label: "admin.suggestedReplyInstructions",
    group: "tickets",
    capabilities: ["tickets.configure"],
  },
  {
    to: "/administration/companies",
    label: "admin.companies",
    group: "organisation",
    capabilities: ["companies.manage"],
  },
  {
    to: "/administration/teams",
    label: "admin.teams",
    group: "organisation",
    capabilities: ["teams.manage"],
  },
  {
    to: "/administration/departments",
    label: "admin.departments",
    group: "organisation",
    capabilities: ["departments.manage"],
  },
  {
    to: "/administration/cost-centres",
    label: "admin.costCentres",
    group: "organisation",
    capabilities: ["companies.manage", "departments.manage"],
  },
  {
    to: "/administration/roles",
    label: "admin.roles",
    group: "organisation",
    capabilities: ["roles.manage"],
  },
  {
    to: "/administration/asset-types",
    label: "admin.assetTypes",
    group: "assets",
    capabilities: ["assets.configure"],
    module: "assets",
  },
  {
    to: "/administration/asset-statuses",
    label: "admin.assetStatuses",
    group: "assets",
    capabilities: ["assets.configure"],
    module: "assets",
  },
  {
    to: "/administration/asset-locations",
    label: "admin.assetLocations",
    group: "assets",
    capabilities: ["assets.configure"],
    module: "assets",
  },
  {
    to: "/administration/supplier-types",
    label: "admin.supplierTypes",
    group: "contracts",
    capabilities: ["contracts.configure"],
    module: "contracts",
  },
  {
    to: "/administration/supplier-statuses",
    label: "admin.supplierStatuses",
    group: "contracts",
    capabilities: ["contracts.configure"],
    module: "contracts",
  },
  {
    to: "/administration/contract-statuses",
    label: "admin.contractStatuses",
    group: "contracts",
    capabilities: ["contracts.configure"],
    module: "contracts",
  },
  {
    to: "/administration/payment-schedules",
    label: "admin.paymentSchedules",
    group: "contracts",
    capabilities: ["contracts.configure"],
    module: "contracts",
  },
  {
    to: "/administration/calendar-categories",
    label: "admin.calendarCategories",
    group: "calendar",
    capabilities: ["calendar.configure"],
    module: "calendar",
  },
];

export const ADMIN_GROUPS: readonly { id: AdminSection["group"]; label: MessageId }[] = [
  { id: "general", label: "admin.group.general" },
  { id: "tickets", label: "admin.group.tickets" },
  { id: "organisation", label: "admin.group.organisation" },
  { id: "assets", label: "admin.group.assets" },
  { id: "contracts", label: "admin.group.contracts" },
  { id: "calendar", label: "admin.group.calendar" },
];
