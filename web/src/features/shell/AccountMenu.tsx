import { useNavigate } from "@tanstack/react-router";
import { ExternalLink, Keyboard, LogOut, UserRound } from "lucide-react";
import { Button } from "@/components/Button";
import { Menu, MenuItem, MenuLabel, MenuRadioGroup, MenuSeparator } from "@/components/Menu";
import { useSession, useSignOut } from "@/features/session/session";
import { useText } from "@/i18n/useText";
import { type Density, setPreferences, type Theme, usePreferences } from "@/lib/preferences";

/** The signed-in Analyst's menu: account, display preferences, help and sign out. */
export function AccountMenu({ onShowShortcuts }: { onShowShortcuts: () => void }) {
  const t = useText();
  const session = useSession();
  const navigate = useNavigate();
  const signOut = useSignOut();
  const prefs = usePreferences();
  const name = session.analyst.full_name;
  const initials = name
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
  return (
    <Menu
      label={t("menu.account", { name })}
      trigger={
        <Button variant="ghost" className="gap-2 px-1.5" title={t("menu.account", { name })}>
          <span
            aria-hidden="true"
            className="grid size-7 place-items-center rounded-full bg-accent-soft text-xs font-semibold text-accent-text"
          >
            {initials}
          </span>
          {/* The visible name is the accessible name (WCAG 2.5.3); narrow screens hide it visually only. */}
          <span className="sr-only max-w-36 truncate text-[0.8125rem] font-medium md:not-sr-only">
            {name}
          </span>
        </Button>
      }
    >
      <MenuLabel>
        <span className="grid gap-0.5">
          <span className="text-sm text-fg">{name}</span>
          <span>{session.analyst.email}</span>
        </span>
      </MenuLabel>
      <MenuSeparator />
      <MenuItem icon={<UserRound />} onSelect={() => void navigate({ to: "/account" })}>
        {t("menu.myAccount")}
      </MenuItem>
      <MenuItem icon={<Keyboard />} shortcut="?" onSelect={onShowShortcuts}>
        {t("menu.shortcuts")}
      </MenuItem>
      <MenuItem icon={<ExternalLink />} onSelect={() => void navigate({ to: "/portal" })}>
        {t("menu.portal")}
      </MenuItem>
      <MenuSeparator />
      <MenuRadioGroup
        label={t("menu.theme")}
        value={prefs.theme}
        onChange={(theme) => setPreferences({ theme: theme as Theme })}
        options={[
          { value: "system", label: t("menu.themeSystem") },
          { value: "light", label: t("menu.themeLight") },
          { value: "dark", label: t("menu.themeDark") },
        ]}
      />
      <MenuRadioGroup
        label={t("menu.density")}
        value={prefs.density}
        onChange={(density) => setPreferences({ density: density as Density })}
        options={[
          { value: "comfortable", label: t("menu.densityComfortable") },
          { value: "compact", label: t("menu.densityCompact") },
        ]}
      />
      <MenuSeparator />
      <MenuItem icon={<LogOut />} onSelect={() => void signOut()}>
        {t("menu.signOut")}
      </MenuItem>
    </Menu>
  );
}
