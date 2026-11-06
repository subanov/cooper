/**
 * The English message catalogue (FR-GEN-009). Every piece of UI text goes through
 * react-intl with an id from here; other locales add a catalogue with the same ids.
 * Terms follow docs/VOCABULARY.md (Analyst, Requester, Company, …).
 */
export const en = {
  "app.name": "Cooper",
  "app.skipToContent": "Skip to content",

  "analyst.title": "Analyst console",
  "home.title": "Home",
  "home.welcome": "Welcome, {name}",
  "home.intro": "Use the navigation or press {shortcut} to jump anywhere.",
  "home.companies":
    "{count, plural, =0 {You see no Companies yet.} one {You see # Company.} other {You see # Companies.}}",
  "home.allCompanies": "You see every Company.",

  "nav.label": "Main",
  "nav.home": "Home",
  "nav.tickets": "Tickets",
  "nav.administration": "Administration",
  "comingSoon.title": "{section}",
  "comingSoon.body": "This screen is on its way.",

  "palette.open": "Search or jump to…",
  "palette.title": "Command palette",
  "palette.placeholder": "Type a command or a screen",
  "palette.empty": "Nothing matches.",
  "palette.navigate": "Go to",
  "palette.actions": "Actions",
  "palette.themeLight": "Use the light theme",
  "palette.themeDark": "Use the dark theme",
  "palette.themeSystem": "Follow the system theme",
  "palette.densityToggle": "Switch density",
  "palette.shortcuts": "Show keyboard shortcuts",
  "palette.signOut": "Sign out",

  "shortcuts.title": "Keyboard shortcuts",
  "shortcuts.description": "Shortcuts work anywhere except while typing in a field.",
  "shortcuts.palette": "Open the command palette",
  "shortcuts.goTo": "Go to {section}",
  "shortcuts.help": "Show this list",

  "menu.account": "Account menu for {name}",
  "menu.theme": "Theme",
  "menu.themeSystem": "System",
  "menu.themeLight": "Light",
  "menu.themeDark": "Dark",
  "menu.density": "Density",
  "menu.densityComfortable": "Comfortable",
  "menu.densityCompact": "Compact",
  "menu.signOut": "Sign out",

  "signIn.title": "Sign in to Cooper",
  "signIn.login": "Username or email",
  "signIn.password": "Password",
  "signIn.code": "Two-factor code",
  "signIn.codeHint": "The 6-digit code from your authenticator app.",
  "signIn.remember": "Remember this browser for 30 days",
  "signIn.submit": "Sign in",
  "signIn.submitting": "Signing in…",
  "signIn.forgot": "Forgot your password?",
  "signIn.invalid": "The username or password is wrong, or the account is inactive.",
  "signIn.codeNeeded": "Enter the code from your authenticator app.",
  "signIn.codeWrong": "That code is wrong or was already used. Try the next one.",
  "signIn.banned":
    "Too many failed sign-ins from this network. Try again in {minutes, plural, one {# minute} other {# minutes}}.",
  "signIn.portal": "Not IT staff? Go to the Self-Service Portal.",

  "reset.title": "Reset your password",
  "reset.intro": "Enter your username or email and we will send you a link.",
  "reset.submit": "Send the link",
  "reset.sent":
    "If the account exists, an email with a link is on its way. The link works for 60 minutes.",
  "reset.newTitle": "Choose a new password",
  "reset.newPassword": "New password",
  "reset.confirmPassword": "Repeat the new password",
  "reset.passwordHint": "At least 12 characters.",
  "reset.mismatch": "The passwords do not match.",
  "reset.save": "Save the password",
  "reset.done": "Your password is changed. Sign in with it now.",
  "reset.invalid": "This link is invalid or has expired. Ask for a new one.",
  "reset.back": "Back to sign-in",

  "totp.title": "Set up two-factor sign-in",
  "totp.required": "Your organisation requires two-factor sign-in. Set it up to continue.",
  "totp.step1":
    "Add this key to an authenticator app (for example Microsoft Authenticator or 1Password).",
  "totp.secret": "Key",
  "totp.openApp": "Open in an authenticator app",
  "totp.step2": "Enter the 6-digit code the app shows.",
  "totp.code": "Code",
  "totp.confirm": "Turn on two-factor sign-in",
  "totp.wrong": "That code is wrong. Check the time on your device and try again.",

  "portal.title": "Self-Service Portal",
  "portal.email": "Email address",
  "portal.actingFor": "Company: {company}",
  "portal.switchCompany": "Act for Company",
  "portal.switchFailed": "Could not switch the Company. Try again.",
  "portalNav.home": "Home",
  "portalNav.tickets": "My tickets",
  "portalHome.welcome": "Welcome, {name}",
  "portalHome.intro": "Raise and follow your tickets with {company}.",
  "portalHome.tickets": "See my tickets",

  "portalSignIn.title": "Sign in",
  "portalSignIn.invalid": "The email address or password is wrong.",
  "portalSignIn.forgot": "Forgot your password, or never set one?",
  "portalSignIn.register": "New here? Register with your work email.",
  "portalSignIn.analyst": "IT staff? Sign in to the Analyst console.",
  "portalReset.title": "Get a password link",
  "portalReset.intro":
    "Enter your email address. If the service desk knows it, we will send you a link to set your password.",
  "portalReset.sent":
    "If the address is known, an email with a link is on its way. The link works for 60 minutes.",
  "portalPassword.title": "Set your password",
  "portalPassword.done": "Your password is set. Sign in with it now.",
  "portalPassword.invalid":
    "This link is invalid, was already used or has expired. Ask for a new one from the sign-in page.",
  "portalPassword.short": "Use at least 12 characters.",
  "portalRegister.title": "Register",
  "portalRegister.intro":
    "Use your work email address. We will send you a link to confirm it and set your password.",
  "portalRegister.name": "Your name",
  "portalRegister.email": "Work email address",
  "portalRegister.submit": "Send the confirmation link",
  "portalRegister.sent": "Check your email: the link to confirm your address works for 24 hours.",
  "portalRegister.off": "Registration is turned off. Contact your service desk for access.",
  "portalRegister.domain":
    "This address does not belong to a Company we support. Use your work email address.",

  "common.loading": "Loading…",
  "common.error": "Something went wrong. Try again.",
  "notFound.title": "Page not found",
  "notFound.back": "Go to the start page",
} as const;

export type MessageId = keyof typeof en;
export const defaultLocale = "en";
