import { Loader2 } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode, Ref } from "react";
import { cn } from "@/lib/cn";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "quiet";
export type ButtonSize = "sm" | "md" | "lg";

const VARIANTS: Record<ButtonVariant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover border border-transparent shadow-raised",
  secondary: "bg-surface text-fg border border-line-strong hover:bg-sunken shadow-raised",
  ghost: "bg-transparent text-fg border border-transparent hover:bg-sunken",
  danger: "bg-danger text-danger-fg border border-transparent hover:brightness-110 shadow-raised",
  quiet: "bg-transparent text-accent-text border border-transparent hover:bg-accent-soft",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "h-8 gap-1.5 px-2.5 text-[0.8125rem]",
  md: "h-9 gap-2 px-3.5 text-sm",
  lg: "h-11 gap-2 px-5 text-[0.9375rem]",
};

/** Classes of a button, for links that look like one (same look, same place: H4). */
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
    "inline-flex shrink-0 select-none items-center justify-center whitespace-nowrap rounded-md font-medium transition-colors",
    "disabled:pointer-events-none disabled:opacity-50 aria-disabled:pointer-events-none aria-disabled:opacity-50",
    "[&_svg]:size-4 [&_svg]:shrink-0",
    VARIANTS[variant],
    SIZES[size],
    className,
  );
}

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant | undefined;
  size?: ButtonSize | undefined;
  /** An icon before the label (decorative: give it aria-hidden). */
  icon?: ReactNode | undefined;
  /** Shows a spinner and blocks repeated clicks while a request runs (H1). */
  loading?: boolean | undefined;
  ref?: Ref<HTMLButtonElement> | undefined;
}

/** A button. Default type is "button", so it never submits a form by accident. */
export function Button({
  variant,
  size,
  icon,
  loading,
  className,
  children,
  disabled,
  type = "button",
  ref,
  ...rest
}: ButtonProps) {
  return (
    <button
      ref={ref}
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={buttonClass({ variant, size, className })}
      {...rest}
    >
      {loading ? <Loader2 aria-hidden="true" className="animate-spin" /> : icon}
      {children}
    </button>
  );
}

export interface IconButtonProps extends Omit<ButtonProps, "children" | "icon"> {
  /** The accessible name, also shown as a tooltip. */
  label: string;
  icon: ReactNode;
  /** Where the tooltip shows; "none" when a visible label sits next to it. */
  tooltip?: "bottom" | "top" | "none" | undefined;
}

/** A square button with only an icon. Its name is the label, shown on hover and focus. */
export function IconButton({
  label,
  icon,
  variant = "ghost",
  size = "md",
  tooltip = "bottom",
  className,
  ...rest
}: IconButtonProps) {
  return (
    <Button
      aria-label={label}
      data-tip={tooltip === "none" ? undefined : label}
      data-tip-side={tooltip === "top" ? "top" : undefined}
      variant={variant}
      size={size}
      icon={icon}
      className={cn(
        tooltip === "none" ? undefined : "tip",
        size === "sm" ? "w-8 px-0" : size === "lg" ? "w-11 px-0" : "w-9 px-0",
        className,
      )}
      {...rest}
    />
  );
}
