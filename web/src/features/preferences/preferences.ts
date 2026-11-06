import { useSyncExternalStore } from "react";

/** Colour theme: follow the operating system, or force one. */
export type Theme = "system" | "light" | "dark";
/** Row and control density: comfortable (default) or compact. */
export type Density = "comfortable" | "compact";

export interface Preferences {
  theme: Theme;
  density: Density;
}

const KEY = "cooper.preferences";
const DEFAULTS: Preferences = { theme: "system", density: "comfortable" };

function read(): Preferences {
  try {
    const raw = window.localStorage.getItem(KEY);
    const parsed = raw ? (JSON.parse(raw) as Partial<Preferences>) : {};
    return {
      theme: ["system", "light", "dark"].includes(parsed.theme ?? "")
        ? (parsed.theme as Theme)
        : DEFAULTS.theme,
      density: parsed.density === "compact" ? "compact" : "comfortable",
    };
  } catch {
    return DEFAULTS;
  }
}

let current: Preferences = typeof window === "undefined" ? DEFAULTS : read();
const listeners = new Set<() => void>();

/** Puts the preferences on <html>: `.dark`/`.light` and `data-density`. */
export function applyPreferences(p: Preferences = current): void {
  const root = document.documentElement;
  root.classList.toggle("dark", p.theme === "dark");
  root.classList.toggle("light", p.theme === "light");
  root.dataset.density = p.density;
}

/**
 * Changes the preferences. They are display settings only, so they live in this
 * browser's localStorage (never tokens or personal data; ADR-0010).
 */
export function setPreferences(change: Partial<Preferences>): void {
  current = { ...current, ...change };
  try {
    window.localStorage.setItem(KEY, JSON.stringify(current));
  } catch {
    // Storage may be unavailable (private mode); the choice still applies now.
  }
  applyPreferences(current);
  for (const listener of listeners) {
    listener();
  }
}

/** The current preferences, re-rendering when they change. */
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
