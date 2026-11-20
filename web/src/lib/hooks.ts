import { useEffect, useRef, useState } from "react";

/** `value`, updated only after it has been stable for `ms` (search-as-you-type). */
export function useDebounced<T>(value: T, ms = 250): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return settled;
}

/** Whether a keyboard event happens while the user types in a field. */
export function isTyping(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement | null;
  if (!target) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable ||
    target.closest("[role=dialog]") !== null
  );
}

/**
 * Single-key shortcuts (`j`, `a`, `?`) and `g` sequences (`g t`). They never fire while
 * typing or in a dialog, and modifier combinations are left to the browser. Every
 * shortcut has a visible alternative (H7: accelerators are never the only way).
 */
export function useHotkeys(bindings: Record<string, () => void>, enabled = true): void {
  const ref = useRef(bindings);
  ref.current = bindings;
  useEffect(() => {
    if (!enabled) return;
    let prefix = "";
    let timer: ReturnType<typeof setTimeout> | undefined;
    function onKey(event: KeyboardEvent) {
      if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
      if (isTyping(event)) return;
      const key = prefix ? `${prefix} ${event.key}` : event.key;
      const action = ref.current[key];
      if (action) {
        event.preventDefault();
        prefix = "";
        action();
        return;
      }
      if (!prefix && Object.keys(ref.current).some((k) => k.startsWith(`${event.key} `))) {
        prefix = event.key;
        clearTimeout(timer);
        timer = setTimeout(() => {
          prefix = "";
        }, 1200);
        return;
      }
      prefix = "";
    }
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      clearTimeout(timer);
    };
  }, [enabled]);
}
