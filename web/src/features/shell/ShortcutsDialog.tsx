import { Fragment } from "react";
import { Dialog } from "@/components/Dialog";
import { Kbd } from "@/components/Layout";
import { useModuleOn, useSession } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { visibleNav } from "./nav";

type Row = [keys: string[], label: string];

/** Lists every keyboard shortcut (`?`), so accelerators are discoverable (H6, H7). */
export function ShortcutsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const t = useText();
  const session = useSession();
  const moduleOn = useModuleOn();
  const global: Row[] = [
    [["Ctrl", "K"], t("shortcuts.palette")],
    [["?"], t("shortcuts.help")],
    [["c"], t("shortcuts.newTicket")],
    ...visibleNav(session, moduleOn).map(
      (item): Row => [["g", item.shortcut], t("shortcuts.goTo", { section: t(item.label) })],
    ),
  ];
  const lists: Row[] = [
    [["j"], t("shortcuts.next")],
    [["k"], t("shortcuts.previous")],
    [["/"], t("shortcuts.search")],
  ];
  const ticket: Row[] = [
    [["a"], t("shortcuts.assignToMe")],
    [["r"], t("shortcuts.reply")],
    [["n"], t("shortcuts.note")],
    [["Ctrl", "Enter"], t("shortcuts.send")],
  ];
  const sections: [string, Row[]][] = [
    [t("shortcuts.global"), global],
    [t("shortcuts.lists"), lists],
    [t("shortcuts.tickets"), ticket],
  ];
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={t("shortcuts.title")}
      description={t("shortcuts.intro")}
      size="lg"
    >
      <div className="grid gap-6 sm:grid-cols-2">
        {sections.map(([heading, rows]) => (
          <section key={heading} className="grid content-start gap-2">
            <h3 className="text-sm font-semibold">{heading}</h3>
            <dl className="grid grid-cols-[max-content_1fr] items-center gap-x-3 gap-y-2 text-sm">
              {rows.map(([keys, label]) => (
                <Fragment key={label}>
                  <dt className="flex items-center gap-1">
                    {keys.map((k, i) => (
                      <Fragment key={k}>
                        {i > 0 && keys[0] === "g" ? (
                          <span className="text-xs text-fg-muted">{t("shortcuts.then")}</span>
                        ) : null}
                        <Kbd>{k}</Kbd>
                      </Fragment>
                    ))}
                  </dt>
                  <dd>{label}</dd>
                </Fragment>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Dialog>
  );
}
