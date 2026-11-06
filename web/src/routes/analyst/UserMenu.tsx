import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  type Density,
  setPreferences,
  type Theme,
  usePreferences,
} from "@/features/preferences/preferences";
import { useText } from "@/i18n/useText";

/** The signed-in Analyst's menu: theme, density and sign-out. */
export function UserMenu({ name, onSignOut }: { name: string; onSignOut: () => void }) {
  const t = useText();
  const { theme, density } = usePreferences();
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" aria-label={t("menu.account", { name })}>
          {name}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel>{t("menu.theme")}</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={theme}
          onValueChange={(v) => setPreferences({ theme: v as Theme })}
        >
          <DropdownMenuRadioItem value="system">{t("menu.themeSystem")}</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="light">{t("menu.themeLight")}</DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="dark">{t("menu.themeDark")}</DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuLabel>{t("menu.density")}</DropdownMenuLabel>
        <DropdownMenuRadioGroup
          value={density}
          onValueChange={(v) => setPreferences({ density: v as Density })}
        >
          <DropdownMenuRadioItem value="comfortable">
            {t("menu.densityComfortable")}
          </DropdownMenuRadioItem>
          <DropdownMenuRadioItem value="compact">{t("menu.densityCompact")}</DropdownMenuRadioItem>
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={onSignOut}>{t("menu.signOut")}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
