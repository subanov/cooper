import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { cn } from "@/lib/cn";
import { Spinner } from "./Spinner";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg shadow-raised hover:brightness-110 active:brightness-95",
  secondary:
    "border border-control/60 bg-surface text-fg shadow-raised hover:bg-sunken active:bg-sunken",
  ghost: "text-fg hover:bg-sunken active:bg-sunken",
  danger: "bg-danger text-danger-fg shadow-raised hover:brightness-110 active:brightness-95",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "h-8 gap-1.5 px-2.5 text-[0.8125rem]",
  md: "h-9 gap-2 px-3.5 text-sm",
  lg: "h-11 gap-2 px-5 text-base",
};

/** Button classes, for links that look like buttons. */
export function buttonClass({
  variant = "secondary",
  size = "md",
  className,
}: {
  variant?: ButtonVariant | undefined;
  size?: ButtonSize | undefined;
  className?: string | undefined;
} = {}): string {
  return cn(
    "inline-flex shrink-0 cursor-pointer select-none items-center justify-center whitespace-nowrap rounded-md font-medium transition-[background-color,filter] disabled:cursor-not-allowed disabled:opacity-55 aria-disabled:cursor-not-allowed aria-disabled:opacity-55",
    VARIANTS[variant],
    SIZES[size],
    className,
  );
}

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant | undefined;
  size?: ButtonSize | undefined;
  /** Shows a spinner and blocks repeated clicks while a request runs (H1). */
  loading?: boolean | undefined;
  /** An icon before the label. */
  icon?: ReactNode;
  ref?: Ref<HTMLButtonElement> | undefined;
}

/**
 * The one button of the design system. One `primary` button per view (Hick's Law);
 * `danger` only for destructive actions, kept apart from frequent ones (Fitts's Law).
 */
export function Button({
  variant,
  size,
  loading = false,
  icon,
  className,
  children,
  disabled,
  type = "button",
  ...props
}: ButtonProps) {
  return (
    <button
      type={type}
      className={buttonClass({ variant, size, className })}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...props}
    >
      {loading ? <Spinner /> : icon}
      {children}
    </button>
  );
}

export interface IconButtonProps extends Omit<ButtonProps, "children" | "icon"> {
  /** The accessible name, also shown as a tooltip on hover and keyboard focus. */
  label: string;
  icon: ReactNode;
  /** Where the tooltip appears. */
  tooltip?: "top" | "bottom" | "none" | undefined;
}

/** A square button with only an icon. Its label is its accessible name and tooltip. */
export function IconButton({
  label,
  icon,
  variant = "ghost",
  size = "md",
  tooltip = "bottom",
  className,
  ...props
}: IconButtonProps) {
  const square = size === "sm" ? "w-8 px-0" : size === "lg" ? "w-11 px-0" : "w-9 px-0";
  return (
    <Button
      variant={variant}
      size={size}
      aria-label={label}
      className={cn("group relative", square, className)}
      {...props}
    >
      {icon}
      {tooltip === "none" ? null : (
        <span
          aria-hidden="true"
          className={cn(
            "pointer-events-none absolute left-1/2 z-50 -translate-x-1/2 whitespace-nowrap rounded-sm bg-fg px-2 py-1 text-xs font-normal text-canvas opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100",
            tooltip === "top" ? "bottom-full mb-1.5" : "top-full mt-1.5",
          )}
        >
          {label}
        </span>
      )}
    </Button>
  );
}
