import { useRouterState } from "@tanstack/react-router";
import { useEffect, useRef } from "react";

/**
 * Moves focus to the new page's heading (`h1`, or the last `[data-page-heading]`) after client-side navigation, so screen reader
 * and keyboard users land on the content (not on the first load).
 */
export function useFocusOnNavigate() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const shownPath = useRef(pathname);
  useEffect(() => {
    if (shownPath.current === pathname) {
      return;
    }
    shownPath.current = pathname;
    // After the new route has rendered: the innermost page heading (a ticket's subject in
    // the split pane), else the page title.
    const frame = requestAnimationFrame(() => {
      const headings = document.querySelectorAll<HTMLElement>(
        "#main [data-page-heading], #main h1",
      );
      headings[headings.length - 1]?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [pathname]);
}
