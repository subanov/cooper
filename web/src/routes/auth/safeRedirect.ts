/**
 * Where to go after sign-in: only a path inside this app, so a crafted link cannot send
 * the Analyst to another site (open redirect).
 */
export function safeRedirect(target: string | undefined): string {
  if (!target?.startsWith("/") || target.startsWith("//") || target.includes("\\")) {
    return "/";
  }
  if (target.startsWith("/sign-in") || target.startsWith("/portal")) {
    return "/";
  }
  return target;
}
