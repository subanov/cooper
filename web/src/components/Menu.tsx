import * as Dropdown from "@radix-ui/react-dropdown-menu";
import { Check } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/cn";

const itemClass =
  "relative flex h-8 cursor-pointer select-none items-center gap-2 rounded-sm px-2 text-sm outline-none data-[disabled]:pointer-events-none data-[highlighted]:bg-sunken data-[disabled]:opacity-50 [&_svg]:size-4 [&_svg]:text-fg-muted";

/** A menu of actions opened from a button (Radix: arrow keys, typeahead, Escape). */
export function Menu({
  trigger,
  children,
  align = "end",
  label,
}: {
  trigger: ReactNode;
  children: ReactNode;
  align?: "start" | "end" | undefined;
  label?: string | undefined;
}) {
  return (
    <Dropdown.Root>
      <Dropdown.Trigger asChild>{trigger}</Dropdown.Trigger>
      <Dropdown.Portal>
        <Dropdown.Content
          align={align}
          sideOffset={6}
          aria-label={label}
          className="z-50 min-w-56 rounded-md border border-line bg-surface p-1 shadow-overlay"
        >
          {children}
        </Dropdown.Content>
      </Dropdown.Portal>
    </Dropdown.Root>
  );
}

export function MenuItem({
  children,
  onSelect,
  icon,
  shortcut,
  disabled,
  tone,
}: {
  children: ReactNode;
  onSelect: () => void;
  icon?: ReactNode;
  shortcut?: ReactNode;
  disabled?: boolean | undefined;
  tone?: "danger" | undefined;
}) {
  return (
    <Dropdown.Item
      onSelect={onSelect}
      disabled={disabled}
      className={cn(itemClass, tone === "danger" && "text-danger-text [&_svg]:text-danger-text")}
    >
      {icon}
      <span className="flex-1">{children}</span>
      {shortcut ? <span className="text-xs text-fg-muted">{shortcut}</span> : null}
    </Dropdown.Item>
  );
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return (
    <Dropdown.Label className="px-2 pb-1 pt-2 text-xs font-medium text-fg-muted">
      {children}
    </Dropdown.Label>
  );
}

export function MenuSeparator() {
  return <Dropdown.Separator className="my-1 h-px bg-line" />;
}

/** A group of mutually exclusive choices inside a menu (theme, density). */
export function MenuRadioGroup({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: readonly { value: string; label: string }[];
}) {
  return (
    <Dropdown.Group>
      <MenuLabel>{label}</MenuLabel>
      <Dropdown.RadioGroup value={value} onValueChange={onChange}>
        {options.map((o) => (
          <Dropdown.RadioItem key={o.value} value={o.value} className={cn(itemClass, "pl-8")}>
            <Dropdown.ItemIndicator className="absolute left-2">
              <Check aria-hidden="true" />
            </Dropdown.ItemIndicator>
            {o.label}
          </Dropdown.RadioItem>
        ))}
      </Dropdown.RadioGroup>
    </Dropdown.Group>
  );
}
