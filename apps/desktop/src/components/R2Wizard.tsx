import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, Folder } from "lucide-react";
import { Modal } from "./Modal";
import { SearchField } from "./SearchField";
import { FallbackImage, levelThumbCandidates, franchisePlaceholder } from "./FallbackImage";
import { Button } from "../ui";
import { Channel } from "@tauri-apps/api/core";
import {
  cacheStatus,
  extractLevelToCache,
  r2CacheNeedsRebuild,
  r2ExtractGlobals,
  r2ExtractLevel,
  r2ImportThumbnail,
  r2LevelOpenPath,
  r2ListMaps,
  r2ReadImportedThumbnail,
  r2ReadScaleformImage,
  r2ReadScaleformImageCrop,
  r2SetupCheck,
  reextractLevelCache,
} from "../api";
import type { CacheEvent } from "../api";
import type {
  R2ExtractEvent,
  R2MapCategory,
  R2MapInfo,
  R2PsarcState,
  R2SetupStatus,
} from "../api";

type Phase = "usrdir" | "globals" | "maps" | "level" | "prepare";

interface R2WizardProps {
  open: boolean;
  busy: boolean;
  onClose: () => void;
  onOpen: (folderPath: string, opts?: { skipCachePrompt?: boolean }) => void;
  /** Game id used for the breadcrumb title + recent-folders key
   *  (defaults to "r2"). Any V2 game with the same USRDIR layout works:
   *  packed/game/global_cached.psarc + packed/levels/. */
  gameId?: string;
  /** Short label shown in the wizard's title breadcrumb
   *  (e.g. "R2", "R3", "ACiT", "A4O"). Defaults to "R2". */
  gameLabel?: string;
}

const USRDIR_KEY_PREFIX = "rechimera.usrdir.last";

function usrdirKey(gameId: string): string {
  return `${USRDIR_KEY_PREFIX}.${gameId}`;
}

function loadUsrdir(gameId: string): string {
  try {
    // Per-game storage first; fall back to the legacy R2-only key so
    // existing users don't lose their saved path on upgrade.
    return (
      localStorage.getItem(usrdirKey(gameId)) ??
      (gameId === "r2" ? localStorage.getItem("rechimera.r2.lastUsrdir") : null) ??
      ""
    );
  } catch {
    return "";
  }
}

function saveUsrdir(gameId: string, p: string) {
  try {
    localStorage.setItem(usrdirKey(gameId), p);
  } catch {
    /* noop */
  }
}

interface ExtractState {
  total: number;
  current: number;
  lastFile: string;
  psarc: string;
  skipped: string[];
  warnings: string[];
}

const EMPTY_EXTRACT: ExtractState = {
  total: 0,
  current: 0,
  lastFile: "",
  psarc: "",
  skipped: [],
  warnings: [],
};

const CATEGORY_LABEL: Record<R2MapCategory, string> = {
  campaign: "Campaign",
  coop: "Co-op",
  multiplayer: "Multiplayer",
  lobby: "Lobby",
  other: "Other",
};

// Why: dedupe — across the scaleform folder, lots of `_iN.tga` files
// are byte-for-byte duplicates because the dev's SWF extractor wrote
// the same bitmap to every sprite index that referenced it. All 4
// `mainmenu_i*.tga` are the same Chicago skyline, and several
// staging sprites pairwise dup each other. We list only one
// representative per unique content.
//
// Atlas slices: `campaignload_id.dds` is 2048×512 — likely a 4×1
// horizontal strip of chapter cards (~512×512 each). We expose each
// quarter as a virtual entry encoded as `crop:<filename>:<x>,<y>,<w>,<h>`;
// the wizard loader sees the prefix and calls
// r2_read_scaleform_image_crop. If the grid guess is wrong, the
// frontend just lets you pick the whole atlas too — easy to retune.
const ATLAS_BY_CATEGORY: Record<R2MapCategory, string[]> = {
  campaign: ["levelselect_id.tga"],
  coop: [
    "mainmenu_i28.tga",
    "competitivestaging_i39.tga",
    "competitivestaging_i43.tga",
    "coopstaging_i34.tga",
  ],
  multiplayer: [
    "competitivestaging_i39.tga",
    "competitivestaging_i43.tga",
    "coopstaging_i34.tga",
  ],
  lobby: [],
  other: [],
};

interface ScaleformImageRef {
  /** Storage key (same value goes into `mapThumbs`). */
  key: string;
  /** Underlying source file. */
  fileName: string;
  /** What loader to use: scaleform read, scaleform crop, or imported thumb. */
  kind: "scaleform" | "scaleform-crop" | "imported";
  /** Crop rectangle when this is a virtual atlas cell. */
  crop?: { x: number; y: number; w: number; h: number };
}

function parseImageRef(spec: string): ScaleformImageRef {
  if (spec.startsWith("crop:")) {
    const rest = spec.slice("crop:".length);
    // crop:<filename>:<x>,<y>,<w>,<h>
    const colonIdx = rest.lastIndexOf(":");
    if (colonIdx > 0) {
      const fileName = rest.slice(0, colonIdx);
      const parts = rest.slice(colonIdx + 1).split(",").map(Number);
      if (
        parts.length === 4 &&
        parts.every((n) => Number.isFinite(n))
      ) {
        return {
          key: spec,
          fileName,
          kind: "scaleform-crop",
          crop: { x: parts[0]!, y: parts[1]!, w: parts[2]!, h: parts[3]! },
        };
      }
    }
  }
  if (spec.startsWith("imported:")) {
    return {
      key: spec,
      fileName: spec.slice("imported:".length),
      kind: "imported",
    };
  }
  return { key: spec, fileName: spec, kind: "scaleform" };
}

const IMPORTED_THUMBS_KEY = "rechimera.r2.importedThumbs";

function loadImportedThumbs(usrdir: string): string[] {
  if (!usrdir) return [];
  try {
    const raw = localStorage.getItem(`${IMPORTED_THUMBS_KEY}:${usrdir}`);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed.filter((s): s is string => typeof s === "string");
  } catch {
    /* ignore */
  }
  return [];
}

function saveImportedThumbs(usrdir: string, list: string[]): void {
  if (!usrdir) return;
  try {
    localStorage.setItem(
      `${IMPORTED_THUMBS_KEY}:${usrdir}`,
      JSON.stringify(list),
    );
  } catch {
    /* ignore */
  }
}

// Default map_id → scaleform sprite filename for stock R2. These come
// directly from the game files — each entry is the unique 474×206
// chapter-card bitmap from `global_cached/scaleform/`. Render-time
// fallback so cards show up automatically without the user touching
// the picker. Explicit picks in `mapThumbs` always win when set; an
// empty pick (user cleared) falls back to the default too — only way
// to suppress is to assign a different one.

// Per-USRDIR localStorage mapping of mapId → atlas filename. Sprite
// indices don't carry their chapter name, so the user assigns each
// once and the choice sticks per install.
const MAP_THUMBS_KEY = "rechimera.r2.mapThumbnails";

function thumbStorageKey(usrdir: string): string {
  return `${MAP_THUMBS_KEY}:${usrdir}`;
}

function loadMapThumbs(usrdir: string): Record<string, string> {
  if (!usrdir) return {};
  try {
    const raw = localStorage.getItem(thumbStorageKey(usrdir));
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object") return parsed as Record<string, string>;
  } catch {
    /* ignore */
  }
  return {};
}

function saveMapThumbs(usrdir: string, m: Record<string, string>): void {
  if (!usrdir) return;
  try {
    localStorage.setItem(thumbStorageKey(usrdir), JSON.stringify(m));
  } catch {
    /* ignore */
  }
}

function psarcStateBadge(state: R2PsarcState) {
  switch (state) {
    case "ready":
      return <span className="r2-badge r2-badge-ok">Ready</span>;
    case "not_extracted":
      return <span className="r2-badge r2-badge-pending">Not extracted</span>;
    case "missing":
      return <span className="r2-badge r2-badge-missing">Missing</span>;
  }
}

export function R2Wizard({
  open,
  busy,
  onClose,
  onOpen,
  gameId = "r2",
  gameLabel = "R2",
}: R2WizardProps) {
  const [phase, setPhase] = useState<Phase>("usrdir");
  const [usrdir, setUsrdir] = useState<string>(() => loadUsrdir(gameId));
  const [status, setStatus] = useState<R2SetupStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [statusBusy, setStatusBusy] = useState(false);

  const [extractBusy, setExtractBusy] = useState(false);
  const [extract, setExtract] = useState<ExtractState>(EMPTY_EXTRACT);
  const [extractDone, setExtractDone] = useState(false);

  const [maps, setMaps] = useState<R2MapInfo[]>([]);
  const [mapsError, setMapsError] = useState<string | null>(null);
  const [mapsBusy, setMapsBusy] = useState(false);
  const [mapsQuery, setMapsQuery] = useState("");
  const [activeCategory, setActiveCategory] = useState<R2MapCategory>("campaign");
  const [selectedMapId, setSelectedMapId] = useState<string | null>(null);
  const [atlasUrls, setAtlasUrls] = useState<Record<string, string>>({});
  const [pickedMap, setPickedMap] = useState<R2MapInfo | null>(null);
  // Per-map thumbnail assignment (mapId → atlas filename), persisted per USRDIR.
  const [mapThumbs, setMapThumbs] = useState<Record<string, string>>({});
  // The map currently in "pick a thumbnail" mode — when set, clicking any
  // atlas tile assigns it to this mapId.
  const [thumbPickerForMap, setThumbPickerForMap] = useState<string | null>(null);
  // User-imported thumbnails (saved RPCS3 textures, screenshots, etc.).
  // Persisted per-USRDIR — show alongside scaleform sprites in the picker.
  const [importedThumbs, setImportedThumbs] = useState<string[]>([]);
  const [importBusy, setImportBusy] = useState(false);
  const globalsForceRebuildRef = useRef(false);
  const [prepareLabel, setPrepareLabel] = useState<string>("");
  const [prepareCurrent, setPrepareCurrent] = useState(0);
  const [prepareTotal, setPrepareTotal] = useState(0);
  const [prepareMode, setPrepareMode] = useState<"fresh" | "rebuild" | "reuse">("fresh");

  const searchRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (open) {
      setPhase("usrdir");
      setStatusError(null);
      setStatus(null);
      setExtract(EMPTY_EXTRACT);
      setExtractDone(false);
      setExtractBusy(false);
      setMaps([]);
      setMapsError(null);
      setPickedMap(null);
      setMapsQuery("");
      setActiveCategory("campaign");
      setSelectedMapId(null);
      Object.values(atlasUrls).forEach((url) => URL.revokeObjectURL(url));
      setAtlasUrls({});
      globalsForceRebuildRef.current = false;
      setPrepareLabel("");
      setPrepareCurrent(0);
      setPrepareTotal(0);
      setPrepareMode("fresh");
      const saved = loadUsrdir(gameId);
      setUsrdir(saved);
      if (saved) {
        void refreshStatus(saved);
      }
    }

  }, [open]);


  const refreshStatus = useCallback(async (path: string) => {
    if (!path.trim()) {
      setStatus(null);
      return;
    }
    setStatusBusy(true);
    setStatusError(null);
    try {
      const s = await r2SetupCheck(path.trim());
      setStatus(s);
    } catch (e) {
      setStatusError(String(e));
      setStatus(null);
    } finally {
      setStatusBusy(false);
    }
  }, []);

  const handleBrowseUsrdir = useCallback(async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Pick the R2 USRDIR folder",
      });
      if (typeof picked === "string") {
        setUsrdir(picked);
        await refreshStatus(picked);
      }
    } catch (e) {
      setStatusError(`Folder picker failed: ${e}`);
    }
  }, [refreshStatus]);

  const handleUsrdirInput = useCallback((value: string) => {
    setUsrdir(value);
  }, []);

  const handleUsrdirBlur = useCallback(() => {
    void refreshStatus(usrdir);
  }, [refreshStatus, usrdir]);

  const canProceedToGlobals = useMemo(() => {
    if (!status || !status.is_usrdir) return false;
    return status.global_cached !== "missing";
  }, [status]);

  const startGlobals = useCallback(async () => {
    if (!canProceedToGlobals || !usrdir.trim()) return;
    saveUsrdir(gameId, usrdir.trim());
    setPhase("globals");
    setExtractBusy(true);
    setExtractDone(false);
    setExtract({ ...EMPTY_EXTRACT });
    try {
      await r2ExtractGlobals(usrdir.trim(), (e: R2ExtractEvent) => {
        applyExtractEvent(e, setExtract);
        if (e.type === "psarc_done" && !e.skipped) {
          globalsForceRebuildRef.current = true;
        }
        if (e.type === "done") {
          setExtractBusy(false);
          setExtractDone(true);
        }
      });
    } catch (e) {
      setStatusError(String(e));
      setExtractBusy(false);
    }
  }, [canProceedToGlobals, usrdir]);

  const continueToMaps = useCallback(async () => {
    setPhase("maps");
    setMapsBusy(true);
    setMapsError(null);
    try {
      const list = await r2ListMaps(usrdir.trim());
      setMaps(list);
    } catch (e) {
      setMapsError(String(e));
    } finally {
      setMapsBusy(false);
    }
  }, [usrdir]);

  const prepareAndOpen = useCallback(
    async (mapId: string, path: string) => {
      let existingCache = false;
      let cacheStale = false;
      let cacheIncomplete = false;
      try {
        const status = await cacheStatus(path);
        existingCache = status.exists;
        cacheStale = status.stale;
        cacheIncomplete = status.incomplete;
      } catch {
        existingCache = false;
      }

      let mode: "fresh" | "rebuild" | "reuse";
      if (!existingCache) {
        mode = "fresh";
      } else if (cacheStale || cacheIncomplete) {
        mode = "rebuild";
      } else {
        let needsRebuild = globalsForceRebuildRef.current;
        if (!needsRebuild) {
          try {
            needsRebuild = await r2CacheNeedsRebuild(usrdir.trim(), mapId);
          } catch {
            needsRebuild = false;
          }
        }
        mode = needsRebuild ? "rebuild" : "reuse";
      }

      if (mode === "reuse") {
        onOpen(path, { skipCachePrompt: true });
        return;
      }

      setPhase("prepare");
      setPrepareMode(mode);
      setPrepareLabel("starting");
      setPrepareCurrent(0);
      setPrepareTotal(0);

      try {
        const ch = new Channel<CacheEvent>();
        ch.onmessage = (e: CacheEvent) => {
          switch (e.type) {
            case "phase":
              setPrepareLabel(e.phase);
              setPrepareTotal(e.total);
              setPrepareCurrent(0);
              break;
            case "progress":
              setPrepareCurrent(e.current);
              break;
            case "done":
              setPrepareLabel("done");
              break;
            case "error":
              setMapsError(e.message);
              break;
          }
        };
        if (mode === "rebuild") {
          await reextractLevelCache(path, ch);
        } else {
          await extractLevelToCache(path, ch);
        }
      } catch (e) {
        setMapsError(String(e));
        return;
      }

      globalsForceRebuildRef.current = false;
      onOpen(path, { skipCachePrompt: true });
    },
    [usrdir, onOpen],
  );

  const pickMap = useCallback(
    async (m: R2MapInfo) => {
      if (!m.psarc_present && !m.ready) return;
      setMapsError(null);
      setPickedMap(m);
      if (m.ready) {
        try {
          const path = await r2LevelOpenPath(usrdir.trim(), m.id);
          await prepareAndOpen(m.id, path);
        } catch (e) {
          setMapsError(String(e));
        }
        return;
      }
      setPhase("level");
      setExtractBusy(true);
      setExtractDone(false);
      setExtract({ ...EMPTY_EXTRACT });
      try {
        await r2ExtractLevel(usrdir.trim(), m.id, (e: R2ExtractEvent) => {
          applyExtractEvent(e, setExtract);
          if (e.type === "done") {
            setExtractBusy(false);
            setExtractDone(true);
          }
        });
        const path = await r2LevelOpenPath(usrdir.trim(), m.id);
        await prepareAndOpen(m.id, path);
      } catch (e) {
        setMapsError(String(e));
        setExtractBusy(false);
      }
    },
    [usrdir, prepareAndOpen],
  );

  useEffect(() => {
    if (phase !== "maps" || !usrdir.trim()) return;
    const files = [
      ...(ATLAS_BY_CATEGORY[activeCategory] ?? []),
      ...importedThumbs.map((f) => `imported:${f}`),
    ];
    if (files.length === 0) return;
    const missing = files.filter((f) => !(f in atlasUrls));
    if (missing.length === 0) return;
    let cancelled = false;
    void (async () => {
      const updates: Record<string, string> = {};
      for (const spec of missing) {
        const ref = parseImageRef(spec);
        try {
          let blob: Blob;
          if (ref.kind === "scaleform-crop" && ref.crop) {
            blob = await r2ReadScaleformImageCrop(
              usrdir.trim(),
              ref.fileName,
              ref.crop.x,
              ref.crop.y,
              ref.crop.w,
              ref.crop.h,
            );
          } else if (ref.kind === "imported") {
            blob = await r2ReadImportedThumbnail(usrdir.trim(), ref.fileName);
          } else {
            blob = await r2ReadScaleformImage(usrdir.trim(), ref.fileName);
          }
          if (cancelled) return;
          updates[ref.key] = URL.createObjectURL(blob);
        } catch (e) {
          if (!cancelled) {
            console.warn(`[r2-wizard] scaleform image ${ref.key}:`, e);
          }
        }
      }
      if (!cancelled && Object.keys(updates).length > 0) {
        setAtlasUrls((prev) => ({ ...prev, ...updates }));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [phase, activeCategory, usrdir, atlasUrls, importedThumbs]);

  // Hydrate per-map thumbnail assignments + imported thumbnail list
  // whenever the USRDIR changes.
  useEffect(() => {
    setMapThumbs(loadMapThumbs(usrdir));
    setImportedThumbs(loadImportedThumbs(usrdir));
    setThumbPickerForMap(null);
  }, [usrdir]);

  const importThumbnail = useCallback(async () => {
    if (!usrdir.trim() || importBusy) return;
    setImportBusy(true);
    try {
      const picked = await openDialog({
        title: "Import a thumbnail (PNG / JPEG)",
        filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }],
      });
      if (typeof picked !== "string") {
        return;
      }
      const base = picked.split(/[\\/]/).pop() ?? "thumb";
      const label = base.replace(/\.[^.]+$/, "").slice(0, 60);
      const stored = await r2ImportThumbnail(usrdir.trim(), picked, label);
      setImportedThumbs((prev) => {
        if (prev.includes(stored)) return prev;
        const next = [...prev, stored];
        saveImportedThumbs(usrdir, next);
        return next;
      });
    } catch (e) {
      console.warn("[r2-wizard] thumbnail import failed:", e);
    } finally {
      setImportBusy(false);
    }
  }, [usrdir, importBusy]);

  const assignMapThumb = useCallback(
    (mapId: string, filename: string) => {
      setMapThumbs((prev) => {
        const next = { ...prev, [mapId]: filename };
        saveMapThumbs(usrdir, next);
        return next;
      });
      setThumbPickerForMap(null);
    },
    [usrdir],
  );

  const clearMapThumb = useCallback(
    (mapId: string) => {
      setMapThumbs((prev) => {
        const next = { ...prev };
        delete next[mapId];
        saveMapThumbs(usrdir, next);
        return next;
      });
    },
    [usrdir],
  );

  useEffect(() => {
    return () => {
      Object.values(atlasUrls).forEach((url) => URL.revokeObjectURL(url));
    };

  }, []);

  const filteredMaps = useMemo(() => {
    const q = mapsQuery.trim().toLowerCase();
    if (!q) return maps;
    return maps.filter(
      (m) =>
        m.id.toLowerCase().includes(q) ||
        m.display_name.toLowerCase().includes(q),
    );
  }, [maps, mapsQuery]);

  const groupedMaps = useMemo(() => {
    const groups: Record<R2MapCategory, R2MapInfo[]> = {
      campaign: [],
      coop: [],
      multiplayer: [],
      lobby: [],
      other: [],
    };
    for (const m of filteredMaps) groups[m.category].push(m);
    return groups;
  }, [filteredMaps]);

  useEffect(() => {
    if (phase !== "maps") return;
    const list = groupedMaps[activeCategory] ?? [];
    if (list.length === 0) {
      setSelectedMapId(null);
      return;
    }
    const first = list[0];
    if (first && (!selectedMapId || !list.find((m) => m.id === selectedMapId))) {
      setSelectedMapId(first.id);
    }
  }, [phase, activeCategory, groupedMaps, selectedMapId]);

  const stepIndex =
    phase === "usrdir"
      ? 1
      : phase === "globals"
        ? 2
        : phase === "maps"
          ? 3
          : 4;

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="xl"
      title={
        <span>
          {gameLabel} : {phase === "usrdir"
            ? "USRDIR"
            : phase === "globals"
              ? "GLOBALS"
              : phase === "maps"
                ? `LEVELS : ${CATEGORY_LABEL[activeCategory].toUpperCase()}`
                : phase === "level"
                  ? "EXTRACT"
                  : "PREPARE"}
          {" :"}
        </span>
      }
      subtitle={
        <span>
          STEP {stepIndex} / 4 ·{" "}
          {phase === "usrdir"
            ? "point to your USRDIR folder"
            : phase === "globals"
              ? "extracting shared globals"
              : phase === "maps"
                ? "pick a map"
                : phase === "level"
                  ? "extracting level data"
                  : "preparing assets for render"}
        </span>
      }
      subheader={
        <ol className="wizard-stepbar" aria-label="Progress">
          {(["USRDIR", "Globals", "Map", "Level"] as const).map((label, i) => {
            const n = i + 1;
            const active = stepIndex >= n;
            const done = stepIndex > n;
            return (
              <li
                key={label}
                className={`wizard-step${active ? " is-active" : ""}${done ? " is-done" : ""}`}
              >
                <span className="wizard-step-num">{n}</span>
                <span className="wizard-step-label">{label}</span>
              </li>
            );
          })}
        </ol>
      }
      footer={renderFooter()}
    >
      {phase === "usrdir" && (
        <div className="r2-step">
          <div className="source-info">
            <strong className="small">📦 What's a USRDIR?</strong>
            <p className="small dim">
              On a PS3 disc rip it's the folder right under{" "}
              <code>PS3_GAME/</code> — e.g.{" "}
              <code>...\R2 ISO\PS3_GAME\USRDIR\</code>. It contains{" "}
              <code>packed/game/global_cached.psarc</code> and a{" "}
              <code>packed/levels/</code> tree.
            </p>
            <p className="small dim">
              We'll extract shared globals once into the same USRDIR (in
              place), then ask which map to extract.
            </p>
          </div>

          <button
            type="button"
            className="open-level-card"
            onClick={handleBrowseUsrdir}
            disabled={statusBusy}
          >
            <div className="open-level-card-icon" aria-hidden>
              <Folder size={28} strokeWidth={1.5} />
            </div>
            <div className="open-level-card-text">
              <div className="open-level-card-title">Pick USRDIR folder</div>
              <div className="open-level-card-sub small dim">
                We'll remember this for next time
              </div>
            </div>
          </button>

          <label className="open-level-field">
            <span className="open-level-field-label small dim">
              Or paste the path
            </span>
            <input
              type="text"
              value={usrdir}
              onChange={(e) => handleUsrdirInput(e.target.value)}
              onBlur={handleUsrdirBlur}
              onKeyDown={(e) => {
                if (e.key === "Enter") void refreshStatus(usrdir);
              }}
              placeholder="D:\\...\\PS3_GAME\\USRDIR"
              spellCheck={false}
              disabled={statusBusy}
            />
          </label>

          {statusError && <div className="error-banner">{statusError}</div>}

          {status && (
            <div className="r2-status">
              {!status.is_usrdir ? (
                <div className="error-banner">
                  This folder doesn't look like a USRDIR — no{" "}
                  <code>packed/game/</code> or <code>packed/levels/</code>{" "}
                  subdirectory.
                </div>
              ) : (
                <ul className="r2-status-list">
                  <li>
                    <span className="r2-status-label">global_cached.psarc</span>
                    {psarcStateBadge(status.global_cached)}
                  </li>
                  <li>
                    <span className="r2-status-label">
                      global_uncached.psarc
                    </span>
                    {psarcStateBadge(status.global_uncached)}
                  </li>
                  <li>
                    <span className="r2-status-label">levels/</span>
                    <span className="r2-badge r2-badge-info">
                      {status.level_folder_count} folder
                      {status.level_folder_count === 1 ? "" : "s"}
                    </span>
                  </li>
                </ul>
              )}
              {status.is_usrdir &&
                status.global_cached === "missing" && (
                  <div className="error-banner">
                    <code>global_cached.psarc</code> is missing — R2 needs at
                    least this archive to render anything. Pointing at the
                    wrong folder?
                  </div>
                )}
              {status.is_usrdir &&
                status.global_uncached === "missing" &&
                status.global_cached !== "missing" && (
                  <div className="open-level-warning">
                    <code>global_uncached.psarc</code> isn't here. We can
                    proceed, but expect missing weapon / character textures.
                  </div>
                )}
            </div>
          )}
        </div>
      )}

      {phase === "globals" && (
        <div className="r2-step">
          <ExtractProgressView state={extract} done={extractDone} busy={extractBusy} />
          {extractDone && (
            <div
              className="open-level-hint small"
              style={{
                borderColor: "rgba(74, 222, 128, 0.4)",
                background: "rgba(74, 222, 128, 0.06)",
              }}
            >
              ✓ Globals ready. Continue to pick a map.
            </div>
          )}
        </div>
      )}

      {phase === "maps" && (
        <div className="r2-step">
          <div className="r2-maps-toolbar">
            <SearchField
              ref={searchRef}
              value={mapsQuery}
              onChange={setMapsQuery}
              placeholder="Filter maps…"
              ariaLabel="Filter maps"
              hotkey="/"
            />
            <span className="small dim">
              {filteredMaps.length} of {maps.length}
            </span>
          </div>

          {mapsBusy && <div className="dim small">Scanning levels…</div>}
          {mapsError && <div className="error-banner">{mapsError}</div>}

          {!mapsBusy && !mapsError && maps.length === 0 && (
            <div className="dim small">
              No folders under <code>packed/levels/</code>.
            </div>
          )}

          <div
            className="r2-hud-tabs"
            role="tablist"
            aria-label="Map categories"
          >
            {(Object.keys(groupedMaps) as R2MapCategory[]).map((cat) => {
              const list = groupedMaps[cat];
              if (list.length === 0) return null;
              const isActive = activeCategory === cat;
              return (
                <button
                  key={cat}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  className={`r2-hud-tab${isActive ? " is-active" : ""}`}
                  onClick={() => setActiveCategory(cat)}
                >
                  {CATEGORY_LABEL[cat]} ({list.length})
                </button>
              );
            })}
          </div>

          {(() => {
            const list = groupedMaps[activeCategory] ?? [];
            if (list.length === 0) {
              return (
                <div className="dim small" style={{ padding: 20 }}>
                  No maps in {CATEGORY_LABEL[activeCategory]}.
                </div>
              );
            }
            const selected: R2MapInfo = list.find((m) => m.id === selectedMapId) ?? list[0]!;
            const explicitFile = mapThumbs[selected.id];
            const explicitUrl = explicitFile ? atlasUrls[explicitFile] : undefined;
            const candidateUrls = explicitUrl
              ? [explicitUrl]
              : levelThumbCandidates(selected.id, gameId);
            const assignedFile = explicitFile;
            const isPicking = thumbPickerForMap === selected.id;
            const availableSprites = [
              ...(ATLAS_BY_CATEGORY[activeCategory] ?? []),
              ...importedThumbs.map((f) => `imported:${f}`),
            ];
            const stateLabel = selected.ready
              ? "EXTRACTED"
              : selected.psarc_present
                ? "NEEDS EXTRACT"
                : "NO PSARC";
            const canOpen = (selected.psarc_present || selected.ready) && !busy;

            return (
              <div className="r2-hud-shell">
                <div className="r2-hud-list" role="listbox">
                  {list.map((m) => {
                    const disabled = !m.psarc_present && !m.ready;
                    const rowState = m.ready
                      ? "READY"
                      : m.psarc_present
                        ? "PSARC"
                        : "—";
                    return (
                      <button
                        key={m.id}
                        type="button"
                        role="option"
                        aria-selected={selected.id === m.id}
                        className={`r2-map-row${
                          selected.id === m.id ? " is-active" : ""
                        }${disabled ? " is-disabled" : ""}`}
                        onClick={() => setSelectedMapId(m.id)}
                        onDoubleClick={() => {
                          if (!disabled) pickMap(m);
                        }}
                        disabled={disabled}
                        title={
                          m.ready
                            ? "Already extracted — open directly"
                            : m.psarc_present
                              ? "Extract and open"
                              : "No PSARCs found for this map"
                        }
                      >
                        <span className="r2-map-row-name">{m.display_name}</span>
                        <span className="r2-map-row-state">{rowState}</span>
                      </button>
                    );
                  })}
                </div>

                <div className="r2-hud-preview">
                  <div className="r2-hud-preview-art">
                    <FallbackImage
                      candidates={candidateUrls}
                      alt=""
                      placeholder={franchisePlaceholder(gameId)}
                      placeholderLabel="No image available"
                    />
                    <span
                      className="r2-hud-preview-art-letter"
                      aria-hidden
                      style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", pointerEvents: "none", zIndex: -1 }}
                    >
                      {selected.display_name.charAt(0)}
                    </span>
                    <span className="r2-hud-preview-caption">
                      {selected.display_name}
                    </span>
                  </div>

                  <div className="r2-hud-preview-meta">
                    <div className="r2-hud-meta-label">Status</div>
                    <div className="r2-hud-meta-value">{stateLabel}</div>
                    <div className="r2-hud-meta-label">Map ID</div>
                    <div className="r2-hud-meta-value mono">{selected.id}</div>
                  </div>

                  {isPicking ? (
                    <div className="r2-map-card-picker" style={{ position: "static", marginTop: 8 }}>
                      <div className="r2-map-card-picker-head small dim">
                        Pick a thumbnail for {selected.display_name}
                      </div>
                      <div className="r2-map-card-picker-grid">
                        {availableSprites.map((f) => {
                          const url = atlasUrls[f];
                          return (
                            <button
                              key={f}
                              type="button"
                              className="r2-map-card-picker-tile"
                              onClick={() => assignMapThumb(selected.id, f)}
                              title={f}
                            >
                              {url ? (
                                <img src={url} alt={f} />
                              ) : (
                                <span className="dim small">loading…</span>
                              )}
                            </button>
                          );
                        })}
                      </div>
                      <div className="r2-map-card-picker-actions">
                        <button
                          type="button"
                          className="r2-map-card-picker-clear small dim"
                          onClick={() => void importThumbnail()}
                          disabled={importBusy}
                        >
                          {importBusy ? "Uploading…" : "Upload custom…"}
                        </button>
                        {assignedFile && (
                          <button
                            type="button"
                            className="r2-map-card-picker-clear small dim"
                            onClick={() => {
                              clearMapThumb(selected.id);
                              setThumbPickerForMap(null);
                            }}
                          >
                            Clear
                          </button>
                        )}
                        <button
                          type="button"
                          className="r2-map-card-picker-close small dim"
                          onClick={() => setThumbPickerForMap(null)}
                        >
                          Close
                        </button>
                      </div>
                    </div>
                  ) : null}

                  <div className="r2-hud-actions">
                    <button
                      type="button"
                      onClick={() => pickMap(selected)}
                      disabled={!canOpen}
                    >
                      {selected.ready ? "Open Level" : "Extract & Open"}
                    </button>
                    {availableSprites.length > 0 && (
                      <button
                        type="button"
                        onClick={() =>
                          setThumbPickerForMap((cur) =>
                            cur === selected.id ? null : selected.id,
                          )
                        }
                      >
                        {assignedFile ? "Change Thumb" : "Set Thumb"}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })()}
        </div>
      )}

      {phase === "level" && (
        <div className="r2-step">
          {pickedMap && (
            <div className="open-level-game-row">
              <span className="game-card-tag mono">{pickedMap.id}</span>
              <span className="small dim">{pickedMap.display_name}</span>
            </div>
          )}
          <ExtractProgressView state={extract} done={extractDone} busy={extractBusy} />
          {mapsError && <div className="error-banner">{mapsError}</div>}
        </div>
      )}

      {phase === "prepare" && (
        <div className="r2-step">
          {pickedMap && (
            <div className="open-level-game-row">
              <span className="game-card-tag mono">{pickedMap.id}</span>
              <span className="small dim">{pickedMap.display_name}</span>
            </div>
          )}
          <div className="source-info">
            <strong className="small">
              {prepareMode === "rebuild"
                ? "🔄 Rebuilding cache"
                : "⚙ Preparing level"}
            </strong>
            <p className="small dim">
              {prepareMode === "rebuild"
                ? "Globals were re-extracted, so the level's cache is being regenerated to pull in any new shared textures."
                : "Decoding mobys, ties, ufrags and textures into a fast-load cache. Three.js will start rendering as soon as this finishes."}
            </p>
          </div>
          <div className="r2-extract">
            <div className="r2-extract-meta">
              <span className="mono small">{prepareLabel || "starting…"}</span>
              {prepareTotal > 0 && (
                <span className="mono small dim">
                  {prepareCurrent.toLocaleString()} /{" "}
                  {prepareTotal.toLocaleString()}
                </span>
              )}
            </div>
            <div className="load-progress-bar">
              <div
                className="load-progress-fill"
                style={{
                  width: `${
                    prepareTotal > 0
                      ? Math.min(100, (prepareCurrent / prepareTotal) * 100)
                      : 0
                  }%`,
                }}
              />
            </div>
          </div>
          {mapsError && <div className="error-banner">{mapsError}</div>}
        </div>
      )}
    </Modal>
  );

  function renderFooter() {
    if (phase === "usrdir") {
      return (
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            onClick={startGlobals}
            disabled={!canProceedToGlobals || statusBusy || busy}
          >
            {status?.global_cached === "ready" &&
            status?.global_uncached === "ready"
              ? "Continue →"
              : "Extract globals →"}
          </Button>
        </>
      );
    }
    if (phase === "globals") {
      return (
        <>
          <Button
            onClick={() => setPhase("usrdir")}
            disabled={extractBusy}
          >
            <ArrowLeft size={12} strokeWidth={2} /> Back
          </Button>
          <Button onClick={onClose} disabled={extractBusy}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={continueToMaps}
            disabled={!extractDone || busy}
          >
            Pick a map →
          </Button>
        </>
      );
    }
    if (phase === "maps") {
      return (
        <>
          <Button onClick={() => setPhase("usrdir")}>
            <ArrowLeft size={12} strokeWidth={2} /> Back to USRDIR
          </Button>
          <Button onClick={onClose}>Cancel</Button>
        </>
      );
    }
    if (phase === "prepare") {
      return (
        <>
          {mapsError && (
            <Button onClick={() => setPhase("maps")}>
              <ArrowLeft size={12} strokeWidth={2} /> Back to maps
            </Button>
          )}
          <Button onClick={onClose}>Cancel</Button>
        </>
      );
    }
    return (
      <>
        <Button onClick={() => setPhase("maps")} disabled={extractBusy}>
          <ArrowLeft size={12} strokeWidth={2} /> Back to maps
        </Button>
        <Button onClick={onClose} disabled={extractBusy || busy}>
          Cancel
        </Button>
      </>
    );
  }
}

function applyExtractEvent(
  e: R2ExtractEvent,
  setExtract: (updater: (prev: ExtractState) => ExtractState) => void,
) {
  switch (e.type) {
    case "psarc_start":
      setExtract((prev) => ({
        ...prev,
        psarc: e.psarc,
        total: e.total,
        current: 0,
        lastFile: "",
      }));
      break;
    case "psarc_progress":
      setExtract((prev) =>
        prev.psarc === e.psarc
          ? { ...prev, current: e.current, lastFile: e.name }
          : { ...prev, psarc: e.psarc, current: e.current, lastFile: e.name },
      );
      break;
    case "psarc_done":
      setExtract((prev) => ({
        ...prev,
        psarc: e.psarc,
        current: prev.total,
        lastFile: e.skipped ? "(already extracted)" : "(done)",
        skipped: e.skipped ? [...prev.skipped, e.psarc] : prev.skipped,
      }));
      break;
    case "warning":
      setExtract((prev) => ({
        ...prev,
        warnings: [...prev.warnings, e.message],
      }));
      break;
    case "done":

      break;
    case "error":
      setExtract((prev) => ({
        ...prev,
        warnings: [...prev.warnings, `error: ${e.message}`],
      }));
      break;
  }
}

function ExtractProgressView({
  state,
  done,
  busy,
}: {
  state: ExtractState;
  done: boolean;
  busy: boolean;
}) {
  const pct =
    state.total > 0 ? Math.min(100, (state.current / state.total) * 100) : 0;
  return (
    <div className="r2-extract">
      <div className="r2-extract-meta">
        <span className="mono small">
          {state.psarc || (busy ? "starting…" : done ? "done" : "—")}
        </span>
        {state.total > 0 && (
          <span className="mono small dim">
            {state.current.toLocaleString()} / {state.total.toLocaleString()}
          </span>
        )}
      </div>
      <div className="load-progress-bar">
        <div className="load-progress-fill" style={{ width: `${pct}%` }} />
      </div>
      <div className="r2-extract-file mono small dim">
        {state.lastFile || " "}
      </div>
      {state.warnings.length > 0 && (
        <ul className="r2-extract-warnings small">
          {state.warnings.map((w, i) => (
            <li key={i}>⚠ {w}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
