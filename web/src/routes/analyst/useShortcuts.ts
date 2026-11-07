import { useEffect, useRef } from "react";

/** Whether the key press happened while typing in a field. */
export function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

export interface ShortcutHandlers {
  /** ⌘K / Ctrl+K. */
  palette: () => void;
  /** `?`. */
  help: () => void;
  /** `g` then a key, within a second. */
  goTo: (key: string) => void;
}

/**
 * Global keyboard shortcuts of the Analyst console. ⌘K / Ctrl+K works everywhere;
 * single-key shortcuts are ignored while typing in a field.
 */
export function useShortcuts(handlers: ShortcutHandlers): void {
  const ref = useRef(handlers);
  ref.current = handlers;
  useEffect(() => {
    let pendingG = 0;
    function onKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        ref.current.palette();
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey || typing(event.target)) {
        return;
      }
      if (event.key === "?") {
        event.preventDefault();
        ref.current.help();
        return;
      }
      if (pendingG && Date.now() - pendingG < 1000) {
        pendingG = 0;
        ref.current.goTo(event.key.toLowerCase());
        return;
      }
      pendingG = event.key === "g" ? Date.now() : 0;
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}

/**
 * Single-key shortcuts of one screen (for example `j`/`k` in the ticket list). Ignored
 * while typing in a field, with a modifier held, or while a dialog or menu is open.
 */
export function useKeys(keys: Record<string, () => void>): void {
  const ref = useRef(keys);
  ref.current = keys;
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.metaKey || event.ctrlKey || event.altKey || typing(event.target)) {
        return;
      }
      if (document.querySelector('[role="dialog"], [role="menu"]')) {
        return;
      }
      const handler = ref.current[event.key];
      if (handler) {
        event.preventDefault();
        handler();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
