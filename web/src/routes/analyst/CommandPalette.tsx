import { useNavigate } from "@tanstack/react-router";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import type { NavItem } from "@/features/navigation/navigation";
import { setPreferences, usePreferences } from "@/features/preferences/preferences";
import { useText } from "@/i18n/useText";

/** ⌘K / Ctrl+K: jump to any screen the Analyst can see or run an action. */
export function CommandPalette({
  open,
  onOpenChange,
  items,
  onShowShortcuts,
  onSignOut,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: NavItem[];
  onShowShortcuts: () => void;
  onSignOut: () => void;
}) {
  const t = useText();
  const navigate = useNavigate();
  const { density } = usePreferences();
  const run = (action: () => void) => {
    onOpenChange(false);
    action();
  };
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby={undefined}>
        <DialogTitle className="sr-only">{t("palette.title")}</DialogTitle>
        <DialogDescription className="sr-only">{t("palette.placeholder")}</DialogDescription>
        <Command label={t("palette.title")}>
          <CommandInput placeholder={t("palette.placeholder")} />
          <CommandList>
            <CommandEmpty>{t("palette.empty")}</CommandEmpty>
            <CommandGroup heading={t("palette.navigate")}>
              {items.map((item) => (
                <CommandItem
                  key={item.to}
                  value={t(item.label)}
                  onSelect={() => run(() => navigate({ to: item.to }))}
                >
                  {t(item.label)}
                </CommandItem>
              ))}
            </CommandGroup>
            <CommandGroup heading={t("palette.actions")}>
              <CommandItem onSelect={() => run(() => setPreferences({ theme: "light" }))}>
                {t("palette.themeLight")}
              </CommandItem>
              <CommandItem onSelect={() => run(() => setPreferences({ theme: "dark" }))}>
                {t("palette.themeDark")}
              </CommandItem>
              <CommandItem onSelect={() => run(() => setPreferences({ theme: "system" }))}>
                {t("palette.themeSystem")}
              </CommandItem>
              <CommandItem
                onSelect={() =>
                  run(() =>
                    setPreferences({ density: density === "compact" ? "comfortable" : "compact" }),
                  )
                }
              >
                {t("palette.densityToggle")}
              </CommandItem>
              <CommandItem onSelect={() => run(onShowShortcuts)}>
                {t("palette.shortcuts")}
              </CommandItem>
              <CommandItem onSelect={() => run(onSignOut)}>{t("palette.signOut")}</CommandItem>
            </CommandGroup>
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
