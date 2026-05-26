import { useEffect, useState } from "react";

interface FallbackImageProps {
  /** URLs to try in priority order. The component walks the list via
   *  onError and renders the first one that loads. */
  candidates: string[];
  alt?: string;
  className?: string;
  /** Optional URL shown when every entry in `candidates` fails. The
   *  placeholder gets a "no image available" overlay so users know the
   *  level art isn't real. */
  placeholder?: string;
  /** Overlay label shown on top of the placeholder. */
  placeholderLabel?: string;
  onAllFailed?: () => void;
}

/** <img> that tries multiple URLs in order. The first one that loads
 *  is rendered; if all fail, the placeholder (when provided) is shown
 *  with a centered overlay label. */
export function FallbackImage({
  candidates,
  alt = "",
  className,
  placeholder,
  placeholderLabel,
  onAllFailed,
}: FallbackImageProps) {
  const key = candidates.join("|");
  const [idx, setIdx] = useState(0);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setIdx(0);
    setFailed(false);
  }, [key]);

  const allOut = failed || candidates.length === 0;

  if (allOut) {
    if (!placeholder) return null;
    return (
      <span
        className={`fallback-image is-placeholder${className ? ` ${className}` : ""}`}
        role="img"
        aria-label={alt || placeholderLabel || "Image unavailable"}
      >
        <img src={placeholder} alt="" />
        {placeholderLabel && (
          <span className="fallback-image-overlay">{placeholderLabel}</span>
        )}
      </span>
    );
  }

  const src = candidates[idx];
  if (!src) return null;
  return (
    <img
      src={src}
      alt={alt}
      className={className}
      onError={() => {
        if (idx + 1 < candidates.length) {
          setIdx(idx + 1);
        } else {
          setFailed(true);
          onAllFailed?.();
        }
      }}
    />
  );
}

/** Same per-game/per-level convention used everywhere in the app:
 *
 *  - Primary: `/<gameId>/maps/<mapId>.png`
 *  - SP fallback for coop/MP: `/<gameId>/maps/<stem>.png`
 *    where `<stem>` is `<mapId>` with `_coop` / `_multiplayer` (and any
 *    trailing `_N`) stripped.
 *
 *  Drop a PNG in `public/<gameId>/maps/<level_folder>.png` and it shows
 *  up — no registration, no curated list, no per-game special cases. */
export function levelThumbCandidates(mapId: string, gameId: string): string[] {
  const id = mapId.toLowerCase();
  const stem = id.replace(/_(multiplayer|coop)(?:_.*)?$/, "");
  const out = [`/${gameId}/maps/${id}.png`];
  if (stem !== id) out.push(`/${gameId}/maps/${stem}.png`);
  return out;
}

/** Convenience for callers that have a full folder path (e.g. recents
 *  list) instead of just the map id. Extracts the last path segment. */
export function levelThumbCandidatesFromPath(folderPath: string, gameId: string): string[] {
  const norm = folderPath.replace(/\\/g, "/").replace(/\/+$/, "");
  const last = norm.split("/").pop() ?? "";
  if (!last) return [];
  return levelThumbCandidates(last, gameId);
}

/** Franchise-wide placeholder image — used when the per-level PNG
 *  doesn't exist. Resistance games (r1/r2/r3) share `r_notset.webp`;
 *  R&C games (anything with `rc_` prefix) share `r&c_notset.jpeg`. */
export function franchisePlaceholder(gameId: string): string {
  if (gameId.startsWith("rc_")) {
    // `&` in URL paths is technically legal but ambiguous; encode it.
    return `/${encodeURIComponent("r&c_notset.jpeg")}`;
  }
  return "/r_notset.webp";
}
