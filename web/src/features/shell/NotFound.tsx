import { Link } from "@tanstack/react-router";
import { Compass } from "lucide-react";
import { buttonClass } from "@/components/Button";
import { EmptyState } from "@/components/Feedback";
import { useText } from "@/i18n/useText";

/** Shown for unknown addresses, inside or outside the signed-in frame. */
export function NotFound() {
  const t = useText();
  return (
    <main id="main" className="grid min-h-[60vh] place-items-center p-6">
      <div>
        <h1 tabIndex={-1} data-page-heading className="sr-only">
          {t("notFound.title")}
        </h1>
        <EmptyState
          icon={<Compass aria-hidden="true" />}
          title={t("notFound.title")}
          action={
            <Link to="/" className={buttonClass({ variant: "primary" })}>
              {t("notFound.home")}
            </Link>
          }
        >
          {t("notFound.body")}
        </EmptyState>
      </div>
    </main>
  );
}
