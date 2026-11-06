/**
 * Where to go after sign-in: only a path inside the same area of this app (Analyst
 * console or Portal), so a crafted link cannot send anyone to another site (open
 * redirect) or across areas.
 */
export function safeRedirect(
  target: string | undefined,
  area: "analyst" | "portal" = "analyst",
): string {
  const home = area === "portal" ? "/portal" : "/";
  if (!target?.startsWith("/") || target.startsWith("//") || target.includes("\\")) {
    return home;
  }
  const inPortal = target === "/portal" || /^\/portal[/?#]/.test(target);
  if (area === "analyst" ? inPortal || target.startsWith("/sign-in") : !inPortal) {
    return home;
  }
  if (area === "portal" && PORTAL_PUBLIC.some((p) => target.startsWith(p))) {
    return home;
  }
  return target;
}

/** Signed-out Portal screens: never a destination after sign-in. */
const PORTAL_PUBLIC = [
  "/portal/sign-in",
  "/portal/reset-password",
  "/portal/set-password",
  "/portal/register",
];
