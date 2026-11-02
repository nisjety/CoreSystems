type ConsentManager = Readonly<{ renew?: () => void }>;

/** Reopen the installed CMP; never submit or invent a user's consent choice. */
export function openCookiePreferences(manager?: ConsentManager): boolean {
  if (typeof manager?.renew !== "function") return false;
  try {
    manager.renew();
    return true;
  } catch {
    return false;
  }
}
