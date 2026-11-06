import { Link } from "@tanstack/react-router";
import { useText } from "@/i18n/useText";

const LINK = "text-sm text-primary underline-offset-4 hover:underline";

/** Back to the Portal sign-in screen. */
export function BackToPortalSignIn() {
  const t = useText();
  return (
    <Link to="/portal/sign-in" className={LINK}>
      {t("reset.back")}
    </Link>
  );
}
