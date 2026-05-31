import { useCallback, useEffect, useState } from "react";
import { APP_VERSION } from "./version";

// localStorage key that records the last app version for which the user
// has already seen the "What's new" modal. On boot we compare it to
// APP_VERSION; if they differ (fresh install OR upgrade), open the modal.
//
// Storing the version (not a boolean) means upgrading 0.5.1 -> 0.6.0 -> 0.7.0
// each triggers the modal exactly once, even if the user installed every
// release. The user can also dismiss without marking — `remindLater` keeps
// the stored version pinned to whatever was there before.
const STORAGE_KEY = "rechimera:lastSeenReleaseNotesVersion";

interface WhatsNewState {
  open: boolean;
  close: () => void;
  showManually: () => void;
}

export function useWhatsNew(): WhatsNewState {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    // On mount, decide whether this is a fresh install / upgrade that
    // hasn't been acknowledged. The check has to be sync against
    // localStorage so we don't flash the modal on every reload.
    let last: string | null = null;
    try {
      last = window.localStorage.getItem(STORAGE_KEY);
    } catch {
      // localStorage can throw in private-browsing / sandboxed contexts.
      // Treat as "no record" and skip the modal — better silent than crash.
      return;
    }
    if (last !== APP_VERSION) {
      setOpen(true);
    }
  }, []);

  const close = useCallback(() => {
    // Any close action (Got it / Escape / backdrop / X) pins the current
    // version so the user doesn't see the same notes again next launch.
    // If they want to re-read, Help → "What's new" reopens it manually.
    try {
      window.localStorage.setItem(STORAGE_KEY, APP_VERSION);
    } catch {
      // Ignore — worst case the modal shows again next launch.
    }
    setOpen(false);
  }, []);

  const showManually = useCallback(() => {
    setOpen(true);
  }, []);

  return { open, close, showManually };
}
