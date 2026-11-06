import { useRouterState } from "@tanstack/react-router";
import { useEffect, useRef } from "react";

/**
 * Moves focus to the new page's heading after client-side navigation, so screen reader
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
    document.querySelector<HTMLElement>("#main h1")?.focus();
  }, [pathname]);
}
