import { CheckCircle2, X, XCircle } from "lucide-react";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { useText } from "@/i18n/useText";
import { cn } from "@/lib/cn";

export interface ToastInput {
  message: string;
  tone?: "success" | "danger" | "info" | undefined;
  /** A reversible action offers Undo instead of asking for confirmation first (H3). */
  action?: { label: string; onClick: () => void } | undefined;
}

interface ToastItem extends ToastInput {
  id: number;
}

const ToastContext = createContext<(toast: ToastInput) => void>(() => {});

/** Shows short confirmations ("Saved.") and Undo offers after a mutation. */
export function useToast(): (toast: ToastInput) => void {
  return useContext(ToastContext);
}

/**
 * Holds the toast region. It is a polite live region, so confirmations are announced
 * without moving focus. Toasts pause while hovered or focused (WCAG 2.2.1).
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const next = useRef(1);
  const show = useCallback((toast: ToastInput) => {
    const id = next.current++;
    setItems((current) => [...current.slice(-2), { ...toast, id }]);
  }, []);
  const dismiss = useCallback((id: number) => {
    setItems((current) => current.filter((t) => t.id !== id));
  }, []);
  return (
    <ToastContext.Provider value={show}>
      {children}
      <section
        aria-live="polite"
        aria-relevant="additions"
        className="pointer-events-none fixed inset-x-0 bottom-0 z-[60] flex flex-col items-center gap-2 p-4 sm:items-end"
      >
        {items.map((item) => (
          <ToastView key={item.id} item={item} onDismiss={() => dismiss(item.id)} />
        ))}
      </section>
    </ToastContext.Provider>
  );
}

function ToastView({ item, onDismiss }: { item: ToastItem; onDismiss: () => void }) {
  const t = useText();
  const [paused, setPaused] = useState(false);
  useEffect(() => {
    if (paused) return;
    const timer = setTimeout(onDismiss, item.action ? 10_000 : 5_000);
    return () => clearTimeout(timer);
  }, [paused, item.action, onDismiss]);
  return (
    // biome-ignore lint/a11y/useSemanticElements: a group of message and buttons, not a form fieldset
    <div
      role="group"
      aria-label={item.message}
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
      className="pointer-events-auto flex w-full max-w-sm items-center gap-3 rounded-lg border border-line bg-surface px-3.5 py-2.5 text-sm shadow-overlay"
    >
      {item.tone === "danger" ? (
        <XCircle aria-hidden="true" className="size-4 shrink-0 text-danger-text" />
      ) : (
        <CheckCircle2
          aria-hidden="true"
          className={cn(
            "size-4 shrink-0",
            item.tone === "info" ? "text-accent-text" : "text-success-text",
          )}
        />
      )}
      <p className="min-w-0 flex-1">{item.message}</p>
      {item.action ? (
        <button
          type="button"
          className="h-8 rounded-md px-2 font-semibold text-accent-text hover:bg-accent-soft"
          onClick={() => {
            item.action?.onClick();
            onDismiss();
          }}
        >
          {item.action.label}
        </button>
      ) : null}
      <button
        type="button"
        aria-label={t("common.dismiss")}
        className="grid size-8 place-items-center rounded-md text-fg-muted hover:bg-sunken"
        onClick={onDismiss}
      >
        <X aria-hidden="true" className="size-4" />
      </button>
    </div>
  );
}
