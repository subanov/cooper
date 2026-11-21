import { administration } from "./en/administration";
import { assets } from "./en/assets";
import { auth } from "./en/auth";
import { calendar } from "./en/calendar";
import { common } from "./en/common";
import { contracts } from "./en/contracts";
import { home } from "./en/home";
import { knowledge } from "./en/knowledge";
import { morningChecks } from "./en/morningChecks";
import { portal } from "./en/portal";
import { serviceStatus } from "./en/serviceStatus";
import { shell } from "./en/shell";
import { tasks } from "./en/tasks";
import { tickets } from "./en/tickets";
import { users } from "./en/users";

/**
 * The English message catalogue (FR-GEN-009). Every piece of UI text goes through it;
 * another locale adds a catalogue with the same ids. Terms follow docs/VOCABULARY.md
 * (Analyst, Requester, Company, …), never their forbidden synonyms.
 */
export const en = {
  ...common,
  ...shell,
  ...auth,
  ...home,
  ...tickets,
  ...tasks,
  ...assets,
  ...calendar,
  ...contracts,
  ...knowledge,
  ...serviceStatus,
  ...morningChecks,
  ...users,
  ...administration,
  ...portal,
} as const;

export type MessageId = keyof typeof en;
export const defaultLocale = "en";
