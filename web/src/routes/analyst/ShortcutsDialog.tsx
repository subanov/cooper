import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import type { NavItem } from "@/features/navigation/navigation";
import { useText } from "@/i18n/useText";

function Keys({ keys }: { keys: string[] }) {
  return (
    <span className="flex gap-1">
      {keys.map((k) => (
        <kbd key={k} className="rounded border border-border bg-muted px-1.5 font-mono text-xs">
          {k}
        </kbd>
      ))}
    </span>
  );
}

/** `?`: the list of keyboard shortcuts. */
export function ShortcutsDialog({
  open,
  onOpenChange,
  items,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  items: NavItem[];
}) {
  const t = useText();
  const rows: [string[], string][] = [
    [["Ctrl", "K"], t("shortcuts.palette")],
    ...items.map((i): [string[], string] => [
      ["g", i.shortcut],
      t("shortcuts.goTo", { section: t(i.label) }),
    ]),
    [["?"], t("shortcuts.help")],
  ];
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="p-6">
        <DialogTitle>{t("shortcuts.title")}</DialogTitle>
        <DialogDescription className="mb-4">{t("shortcuts.description")}</DialogDescription>
        <dl className="grid gap-2 text-sm">
          {rows.map(([keys, label]) => (
            <div key={label} className="flex items-center justify-between gap-4">
              <dt>{label}</dt>
              <dd>
                <Keys keys={keys} />
              </dd>
            </div>
          ))}
        </dl>
      </DialogContent>
    </Dialog>
  );
}
