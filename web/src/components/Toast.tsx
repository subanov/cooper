import { CheckCircle2, Info, TriangleAlert, X } from "lucide-react";
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
  tone?: "success" | "info" | "danger" | undefined;
  /** One action, typically Undo (H3: undo instead of confirming). */
  action?: { label: string; onClick: () => void } | undefined;
}

interface ToastItem extends ToastInput {
  id: number;
}

const ToastContext = createContext<(toast: ToastInput) => void>(() => {});

/**
 * Shows a short message after an action (H1). The live regions stay mounted so screen
 * readers announce what is added; errors are assertive. They carry no role, so a page's own
 * status and alert messages remain the only ones a test or assistive tool finds by role.
 */
export function useToast(): (toast: ToastInput) => void {
  return useContext(ToastContext);
}

let nextId = 1;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const dismiss = useCallback(
    (id: number) => setToasts((all) => all.filter((t) => t.id !== id)),
    [],
  );
  const show = useCallback((toast: ToastInput) => {
    const id = nextId++;
    setToasts((all) => [...all.slice(-3), { ...toast, id }]);
  }, []);
  return (
    <ToastContext.Provider value={show}>
      {children}
      <div className="pointer-events-none fixed bottom-4 left-1/2 z-[80] grid w-[min(28rem,calc(100vw-2rem))] -translate-x-1/2 gap-2 sm:left-auto sm:right-4 sm:translate-x-0">
        <div aria-live="polite" className="grid gap-2">
          {toasts
            .filter((t) => t.tone !== "danger")
            .map((t) => (
              <ToastView key={t.id} toast={t} onDismiss={() => dismiss(t.id)} />
            ))}
        </div>
        <div aria-live="assertive" className="grid gap-2">
          {toasts
            .filter((t) => t.tone === "danger")
            .map((t) => (
              <ToastView key={t.id} toast={t} onDismiss={() => dismiss(t.id)} />
            ))}
        </div>
      </div>
    </ToastContext.Provider>
  );
}

function ToastView({ toast, onDismiss }: { toast: ToastItem; onDismiss: () => void }) {
  const t = useText();
  const [paused, setPaused] = useState(false);
  const ms = toast.action ? 10_000 : toast.tone === "danger" ? 12_000 : 5_000;
  const left = useRef(ms);
  useEffect(() => {
    if (paused) return;
    const started = Date.now();
    const timer = setTimeout(onDismiss, left.current);
    return () => {
      clearTimeout(timer);
      left.current -= Date.now() - started;
    };
  }, [paused, onDismiss]);
  const Icon =
    toast.tone === "danger" ? TriangleAlert : toast.tone === "success" ? CheckCircle2 : Info;
  return (
    // Pausing on hover and focus gives time to read and to reach Undo (WCAG 2.2.1).
    // biome-ignore lint/a11y/noStaticElementInteractions: hover only pauses the timer
    <div
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
      className={cn(
        "pointer-events-auto flex items-start gap-2.5 rounded-lg border bg-raised px-3.5 py-3 text-sm shadow-overlay",
        toast.tone === "danger" ? "border-danger-text/50" : "border-line",
      )}
    >
      <Icon
        aria-hidden="true"
        className={cn(
          "mt-0.5 size-4 shrink-0",
          toast.tone === "danger"
            ? "text-danger-text"
            : toast.tone === "success"
              ? "text-success-text"
              : "text-info-text",
        )}
      />
      <p className="min-w-0 flex-1 [overflow-wrap:anywhere]">{toast.message}</p>
      {toast.action ? (
        <button
          type="button"
          onClick={() => {
            toast.action?.onClick();
            onDismiss();
          }}
          className="shrink-0 rounded px-1.5 font-semibold text-accent-text hover:underline"
        >
          {toast.action.label}
        </button>
      ) : null}
      <button
        type="button"
        aria-label={t("common.dismiss")}
        onClick={onDismiss}
        className="-m-1 grid size-7 shrink-0 place-items-center rounded text-fg-muted hover:bg-sunken hover:text-fg"
      >
        <X aria-hidden="true" className="size-4" />
      </button>
    </div>
  );
}
