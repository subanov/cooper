import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

const item =
  "flex h-9 cursor-pointer select-none items-center gap-2.5 rounded px-2.5 text-sm outline-none data-[disabled]:pointer-events-none data-[disabled]:opacity-50 data-[highlighted]:bg-sunken [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-fg-muted";

/** A menu of actions behind a trigger (arrow keys, type-ahead, Escape). */
export function Menu({
  label,
  trigger,
  children,
  align = "end",
}: {
  label: string;
  trigger: ReactNode;
  children: ReactNode;
  align?: "start" | "end" | undefined;
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          aria-label={label}
          align={align}
          sideOffset={6}
          className="z-50 min-w-52 max-w-80 rounded-md border border-line bg-raised p-1 shadow-overlay"
        >
          {children}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

export function MenuItem({
  icon,
  tone,
  onSelect,
  disabled,
  children,
}: {
  icon?: ReactNode | undefined;
  tone?: "danger" | undefined;
  onSelect: () => void;
  disabled?: boolean | undefined;
  children: ReactNode;
}) {
  return (
    <DropdownMenu.Item
      disabled={disabled}
      onSelect={onSelect}
      className={cn(item, tone === "danger" && "text-danger-text [&_svg]:text-danger-text")}
    >
      {icon ? <span aria-hidden="true">{icon}</span> : null}
      <span className="min-w-0 flex-1 truncate">{children}</span>
    </DropdownMenu.Item>
  );
}

export function MenuSeparator() {
  return <DropdownMenu.Separator className="my-1 h-px bg-line" />;
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return (
    <DropdownMenu.Label className="px-2.5 pb-1 pt-2 text-xs font-semibold text-fg-muted">
      {children}
    </DropdownMenu.Label>
  );
}

/** Mutually exclusive choices inside a menu (theme, density). */
export function MenuRadio<T extends string>({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: T;
  onChange: (value: T) => void;
  options: readonly { value: T; label: string }[];
}) {
  return (
    <DropdownMenu.RadioGroup
      aria-label={label}
      value={value}
      onValueChange={(v) => onChange(v as T)}
    >
      {options.map((o) => (
        <DropdownMenu.RadioItem key={o.value} value={o.value} className={item}>
          <span className="grid size-4 place-items-center">
            <DropdownMenu.ItemIndicator>
              <Check aria-hidden="true" />
            </DropdownMenu.ItemIndicator>
          </span>
          {o.label}
        </DropdownMenu.RadioItem>
      ))}
    </DropdownMenu.RadioGroup>
  );
}
