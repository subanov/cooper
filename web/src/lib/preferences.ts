import { useSyncExternalStore } from "react";

/** Colour theme: follow the operating system, or force one. */
export type Theme = "system" | "light" | "dark";
/** Control and row density. */
export type Density = "comfortable" | "compact";

/** The Analyst navigation: labels shown, or icons only for more room. */
export type NavMode = "expanded" | "collapsed";

export interface Preferences {
  theme: Theme;
  density: Density;
  nav: NavMode;
}

const KEY = "cooper.preferences";
const DEFAULTS: Preferences = { theme: "system", density: "comfortable", nav: "expanded" };

function read(): Preferences {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(KEY) ?? "{}") as Partial<Preferences>;
    return {
      theme: parsed.theme === "light" || parsed.theme === "dark" ? parsed.theme : "system",
      density: parsed.density === "compact" ? "compact" : "comfortable",
      nav: parsed.nav === "collapsed" ? "collapsed" : "expanded",
    };
  } catch {
    return DEFAULTS;
  }
}

let current: Preferences = typeof window === "undefined" ? DEFAULTS : read();
const listeners = new Set<() => void>();

/** Puts the preferences on <html> (`.dark`/`.light`, `data-density`). */
export function applyPreferences(p: Preferences = current): void {
  const root = document.documentElement;
  root.classList.toggle("dark", p.theme === "dark");
  root.classList.toggle("light", p.theme === "light");
  root.dataset.density = p.density;
}

/**
 * Changes display preferences. They are not secret, so they live in this browser's
 * localStorage; sessions and tokens never do (ADR-0010).
 */
export function setPreferences(change: Partial<Preferences>): void {
  current = { ...current, ...change };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Storage can be unavailable (private mode); the choice still applies now.
  }
  applyPreferences(current);
  for (const listener of listeners) listener();
}

/** The current preferences; re-renders when they change. */
export function usePreferences(): Preferences {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
    () => DEFAULTS,
  );
}
