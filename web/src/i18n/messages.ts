/**
 * The English message catalogue (FR-GEN-009). Every piece of UI text goes through
 * react-intl with an id from here; other locales add a catalogue with the same ids.
 * Terms follow docs/VOCABULARY.md (Analyst, Requester, Company, …).
 */
export const en = {
  "app.name": "Cooper",
  "app.skipToContent": "Skip to content",
  "analyst.title": "Analyst console",
  "analyst.signedInAs": "Signed in as {name}",
  "analyst.notSignedIn": "You are not signed in.",
  "analyst.signInSoon": "Sign-in screens arrive with the Analyst UI shell.",
  "portal.title": "Self-Service Portal",
  "portal.signedInAs": "Signed in as {name} ({company})",
  "portal.notSignedIn": "Sign in to raise and follow your tickets.",
  "common.loading": "Loading…",
  "common.error": "Something went wrong. Try again.",
  "notFound.title": "Page not found",
  "notFound.back": "Go to the start page",
} as const;

export type MessageId = keyof typeof en;
export const defaultLocale = "en";
