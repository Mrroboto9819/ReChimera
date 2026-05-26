import { useCallback, useEffect, useRef } from "react";
import { useAppSelector } from "./store";

/** Logical UI events that can be sounded. Each skin folder under
 *  `public/ui-effects/<skin>/` may contain `<event>.<ext>` files
 *  for any of these — missing files are silently no-ops. */
export type UiSoundEvent =
  | "select"
  | "hover"
  | "back"
  | "confirm"
  | "error"
  | "skin-switch"
  | "modal-open";

const EXTS = ["wav", "ogg", "mp3"] as const;

// Module-level caches so multiple components share loaded buffers.
const audioCache = new Map<string, HTMLAudioElement>();
const missCache = new Set<string>();
const inflight = new Map<string, Promise<HTMLAudioElement | null>>();

function loadSound(skin: string, event: UiSoundEvent): Promise<HTMLAudioElement | null> {
  const key = `${skin}:${event}`;
  if (missCache.has(key)) return Promise.resolve(null);
  const cached = audioCache.get(key);
  if (cached) return Promise.resolve(cached);
  const existing = inflight.get(key);
  if (existing) return existing;

  const promise = new Promise<HTMLAudioElement | null>((resolve) => {
    const tryExt = (idx: number) => {
      if (idx >= EXTS.length) {
        missCache.add(key);
        resolve(null);
        return;
      }
      const url = `/ui-effects/${skin}/${event}.${EXTS[idx]}`;
      const audio = new Audio(url);
      audio.preload = "auto";
      const onReady = () => {
        cleanup();
        audioCache.set(key, audio);
        resolve(audio);
      };
      const onFail = () => {
        cleanup();
        tryExt(idx + 1);
      };
      const cleanup = () => {
        audio.removeEventListener("canplaythrough", onReady);
        audio.removeEventListener("loadeddata", onReady);
        audio.removeEventListener("error", onFail);
      };
      audio.addEventListener("canplaythrough", onReady, { once: true });
      audio.addEventListener("loadeddata", onReady, { once: true });
      audio.addEventListener("error", onFail, { once: true });
    };
    tryExt(0);
  });
  inflight.set(key, promise);
  promise.finally(() => inflight.delete(key));
  return promise;
}

/** Hook: returns a `play(event)` function that plays the matching audio
 *  file from the active skin's folder. No-op when sound is disabled in
 *  settings, when no file exists for that event, or when the browser
 *  blocks autoplay (we catch + swallow the rejection). */
export function useUiSound() {
  const skin = useAppSelector((s) => s.settings.appSkin);
  const enabled = useAppSelector((s) => s.settings.uiSoundEnabled);
  const volume = useAppSelector((s) => s.settings.uiSoundVolume);

  const enabledRef = useRef(enabled);
  const volumeRef = useRef(volume);
  const skinRef = useRef(skin);

  useEffect(() => {
    enabledRef.current = enabled;
  }, [enabled]);
  useEffect(() => {
    volumeRef.current = volume;
  }, [volume]);
  useEffect(() => {
    skinRef.current = skin;
  }, [skin]);

  const play = useCallback((event: UiSoundEvent) => {
    if (!enabledRef.current) return;
    const currentSkin = skinRef.current;
    void loadSound(currentSkin, event).then((audio) => {
      if (!audio || !enabledRef.current) return;
      // Clone so rapid repeats (multiple clicks in quick succession)
      // can overlap instead of restarting a single buffer.
      const clone = audio.cloneNode() as HTMLAudioElement;
      clone.volume = volumeRef.current;
      const result = clone.play();
      if (result && typeof result.catch === "function") {
        result.catch(() => {
          /* autoplay blocked or other transient error — swallow */
        });
      }
    });
  }, []);

  return play;
}

/** Set up the global UI sound system once, mounted in App.tsx:
 *  - delegates click events on `button` / `.btn` to play `select` /
 *    `confirm` (primary) / `error` (disabled).
 *  - watches skin changes and plays `skin-switch` on the new skin. */
export function useGlobalUiSounds() {
  const play = useUiSound();
  const skin = useAppSelector((s) => s.settings.appSkin);
  const previousSkinRef = useRef(skin);

  useEffect(() => {
    if (previousSkinRef.current !== skin) {
      previousSkinRef.current = skin;
      play("skin-switch");
    }
  }, [skin, play]);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      if (!target) return;
      const btn = target.closest<HTMLButtonElement>("button, .btn, [role='option']");
      if (!btn) return;
      // Skip elements that explicitly opt out via data-no-sound.
      if (btn.dataset.noSound != null) return;
      if (btn.hasAttribute("disabled") || btn.getAttribute("aria-disabled") === "true") {
        play("error");
        return;
      }
      const isPrimary = btn.classList.contains("btn-primary");
      play(isPrimary ? "confirm" : "select");
    };
    document.addEventListener("click", handler, true);
    return () => document.removeEventListener("click", handler, true);
  }, [play]);
}
