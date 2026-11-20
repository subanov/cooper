import { useRouterState } from "@tanstack/react-router";
import { type RefObject, useEffect, useRef } from "react";

/** Set after the first client-side navigation, so the first page load keeps focus alone. */
let navigated = false;

/**
 * After client-side navigation, moves focus to the new screen's heading (the innermost
 * `[data-page-heading]`), so keyboard and screen reader users land on the content
 * (WCAG 2.4.3). Not on the first load, and not when only the search changes.
 */
export function useFocusOnNavigate(): void {
  const pathname = useRouterState({ select: (s) => s.resolvedLocation?.pathname });
  const shown = useRef(pathname);
  useEffect(() => {
    if (shown.current === pathname) return;
    const first = shown.current === undefined;
    shown.current = pathname;
    if (first) return;
    navigated = true;
    const frame = requestAnimationFrame(() => {
      const headings = document.querySelectorAll<HTMLElement>("#main [data-page-heading]");
      headings[headings.length - 1]?.focus({ preventScroll: false });
    });
    return () => cancelAnimationFrame(frame);
  }, [pathname]);
}

/**
 * A heading that appears after its data loaded takes focus, when focus was lost with the
 * loading placeholder (it sits on <body>). Keeps keyboard users on the content.
 */
export function useClaimFocus(ref: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    if (
      navigated &&
      (document.activeElement === document.body || document.activeElement === null)
    ) {
      ref.current?.focus({ preventScroll: true });
    }
  }, [ref]);
}
