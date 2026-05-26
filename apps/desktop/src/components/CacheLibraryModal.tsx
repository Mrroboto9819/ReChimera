import { Channel, invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  ChevronLeft,
  ChevronRight,
  Database,
  Download,
  Maximize2,
  Minimize2,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  bulkExtractSoundsZip,
  classifySound,
  exportTextureDds,
  exportTexturePng,
  extractOneSound,
  extractOneStreamSound,
  loadCachedTextures,
  readCachedAsset,
  readCachedBytes,
  readCachedManifest,
  reextractLevelCache,
  writeBytes,
  type AssetMeshes,
  type CacheEvent,
  type CacheManifest,
  type CubemapDescriptor,
  type CacheManifestEntry,
  type ExtractedSound,
  type Instance,
  type LevelMeshes,
  type SoundCategory,
  type SoundEntry,
  type TextureBlobMap,
} from "../api";
import { AssetPreview } from "../views/AssetPreview";
import { ExportOptionsModal } from "./ExportOptionsModal";
import type { ExportPicks } from "../views/GlbPreview";
import { Modal } from "./Modal";
import { SoundPlayer, type NowPlaying } from "./SoundPlayer";
import { Button } from "../ui";

interface CacheLibraryModalProps {
  open: boolean;
  onClose: () => void;
  folder: string | null;
  initialAssetTuid?: string | null;
  initialPanel?: LibraryFilter | null;
  initialTextureId?: string | null;
  initialSoundKey?: string | null;
  sounds?: SoundEntry[];
  onRequestExtract?: () => void;
  onUseAsSkybox?: (textureId: number) => void;
  currentSkyboxTextureId?: number | null;
  cubemapDescriptor?: CubemapDescriptor | null;
  /** Sends the currently-previewed asset to the Asset workbench tab in
   *  the center panel. Replaces the in-modal fullscreen toggle so users
   *  get the full layered view + animation scrubber rather than just a
   *  bigger canvas. */
  onOpenInWorkbench?: (assetTuidHex: string, kind: "moby" | "tie") => void;
}

export type LibraryFilter =
  | "moby"
  | "tie"
  | "detail"
  | "sound"
  | "texture"
  | "sky";

interface MobyRow {
  entry: CacheManifestEntry;
  

  leaf: string;
  
  group: string;
}




function splitPath(entry: CacheManifestEntry): { group: string; leaf: string } {
  if (entry.name && entry.name.length > 0) {
    const parts = entry.name.split("/").filter(Boolean);
    if (parts.length === 0) {
      return { group: "(unnamed)", leaf: entry.name };
    }
    if (parts.length === 1) {
      return { group: "(top-level)", leaf: parts[0]! };
    }
    return {
      group: parts.slice(0, -1).join("/"),
      leaf: parts[parts.length - 1]!,
    };
  }
  return { group: "(unnamed)", leaf: `…${entry.tuid.slice(-6)}` };
}












export function CacheLibraryModal({
  open,
  onClose,
  folder,
  initialAssetTuid,
  initialPanel,
  initialTextureId,
  initialSoundKey,
  sounds,
  onRequestExtract,
  onUseAsSkybox,
  currentSkyboxTextureId,
  cubemapDescriptor: _cubemapDescriptor,
  onOpenInWorkbench,
}: CacheLibraryModalProps) {
  const [manifest, setManifest] = useState<CacheManifest | null>(null);
  const [manifestError, setManifestError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<LibraryFilter>("moby");
  const [selectedTuid, setSelectedTuid] = useState<string | null>(null);
  const [selectedAsset, setSelectedAsset] = useState<AssetMeshes | null>(null);
  const [selectedTextures, setSelectedTextures] = useState<TextureBlobMap>(
    () => new Map(),
  );
  const [loadingAsset, setLoadingAsset] = useState(false);
  const [exporting, _setExporting] = useState(false);
  const [reextractStatus, setReextractStatus] = useState<string | null>(null);

  
  useEffect(() => {
    if (!open || !folder) {
      setManifest(null);
      setManifestError(null);
      setSelectedTuid(null);
      setSelectedAsset(null);
      return;
    }
    let cancelled = false;
    setManifest(null);
    setManifestError(null);
    readCachedManifest(folder)
      .then((m) => {
        if (cancelled) return;
        setManifest(m);
      })
      .catch((e) => {
        if (cancelled) return;
        setManifestError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, folder]);

  
  
  
  
  useEffect(() => {
    if (!folder || !selectedTuid || !manifest) {
      setSelectedAsset(null);
      setSelectedTextures(new Map());
      return;
    }
    const entry = manifest.entries.find(
      (e) => (e.kind === "moby" || e.kind === "tie") && e.tuid === selectedTuid,
    );
    if (!entry) {
      setSelectedAsset(null);
      setSelectedTextures(new Map());
      return;
    }
    let cancelled = false;
    setLoadingAsset(true);
    setSelectedTextures(new Map());
    readCachedAsset(folder, entry.file)
      .then(async (data) => {
        if (cancelled) return;
        const asset = data as AssetMeshes;
        setSelectedAsset(asset);
        
        
        
        
        const ids = new Set<number>();
        for (const m of asset.submeshes) {
          for (const id of [m.albedo_id, m.normal_id, m.emissive_id]) {
            if (typeof id === "number") ids.add(id);
          }
        }
        const blobs = await loadCachedTextures(folder, [...ids]);
        if (cancelled) return;
        setSelectedTextures(blobs);
        setLoadingAsset(false);
      })
      .catch(() => {
        if (cancelled) return;
        setSelectedAsset(null);
        setSelectedTextures(new Map());
        setLoadingAsset(false);
      });
    return () => {
      cancelled = true;
    };
  }, [folder, selectedTuid, manifest]);

  
  
  
  const grouped = useMemo(() => {
    if (filter !== "moby" && filter !== "tie" && filter !== "detail") return [];
    if (!manifest) return [];
    const needle = search.trim().toLowerCase();
    const rows: MobyRow[] = [];
    for (const entry of manifest.entries) {
      if (entry.kind !== filter) continue;
      const { group, leaf } = splitPath(entry);
      if (
        needle &&
        !group.toLowerCase().includes(needle) &&
        !leaf.toLowerCase().includes(needle) &&
        !entry.tuid.toLowerCase().includes(needle)
      ) {
        continue;
      }
      rows.push({ entry, leaf, group });
    }
    rows.sort((a, b) => {
      const g = a.group.localeCompare(b.group);
      return g !== 0 ? g : a.leaf.localeCompare(b.leaf);
    });

    const buckets: { group: string; rows: MobyRow[] }[] = [];
    for (const row of rows) {
      const last = buckets[buckets.length - 1];
      if (last && last.group === row.group) {
        last.rows.push(row);
      } else {
        buckets.push({ group: row.group, rows: [row] });
      }
    }
    return buckets;
  }, [manifest, search, filter]);

  const textureRows = useMemo(() => {
    if (filter !== "texture") return [];
    if (!manifest) return [];
    const needle = search.trim().toLowerCase();
    const out: CacheManifestEntry[] = [];
    for (const entry of manifest.entries) {
      if (entry.kind !== "texture") continue;
      if (needle && !entry.tuid.toLowerCase().includes(needle)) continue;
      out.push(entry);
    }
    out.sort((a, b) => {
      const an = parseInt(a.tuid, 10);
      const bn = parseInt(b.tuid, 10);
      if (Number.isFinite(an) && Number.isFinite(bn)) return an - bn;
      return a.tuid.localeCompare(b.tuid);
    });
    return out;
  }, [filter, manifest, search]);

  const [soundCategory, setSoundCategory] = useState<SoundCategory | "all">(
    "all",
  );
  const [bulkSoundZipBusy, setBulkSoundZipBusy] = useState(false);
  const [bulkSoundZipStatus, setBulkSoundZipStatus] = useState<string | null>(null);

  const handleBulkSoundZip = useCallback(async () => {
    if (!folder || bulkSoundZipBusy) return;
    setBulkSoundZipStatus(null);
    const lastSeg = folder
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() || "sounds";
    const suggested = `${lastSeg}-sounds.zip`;
    let outPath: string | null = null;
    try {
      const picked = await saveDialog({
        defaultPath: suggested,
        filters: [{ name: "Zip archive", extensions: ["zip"] }],
        title: "Save all level sounds as ZIP",
      });
      if (typeof picked === "string") outPath = picked;
    } catch (e) {
      setBulkSoundZipStatus(`Picker failed: ${e}`);
      return;
    }
    if (!outPath) return;
    setBulkSoundZipBusy(true);
    try {
      const count = await bulkExtractSoundsZip(folder, outPath);
      setBulkSoundZipStatus(`Wrote ${count} WAVs → ${outPath}`);
    } catch (e) {
      setBulkSoundZipStatus(`Failed: ${e}`);
    } finally {
      setBulkSoundZipBusy(false);
    }
  }, [folder, bulkSoundZipBusy]);

  const soundRows = useMemo(() => {
    if (filter !== "sound") return [];
    if (!sounds) return [];
    const needle = search.trim().toLowerCase();
    return sounds.filter((s) => {
      if (soundCategory !== "all" && classifySound(s.source, s.name) !== soundCategory) {
        return false;
      }
      return needle
        ? s.name.toLowerCase().includes(needle) ||
            s.source.toLowerCase().includes(needle)
        : true;
    });
  }, [filter, sounds, search, soundCategory]);

  const soundCategoryCounts = useMemo(() => {
    const counts = { all: 0, sfx: 0, dialog: 0, music: 0 };
    if (filter !== "sound" || !sounds) return counts;
    for (const s of sounds) {
      counts.all++;
      counts[classifySound(s.source, s.name)]++;
    }
    return counts;
  }, [filter, sounds]);

  // Why: tabs whose underlying dataset is empty just clutter the
  // toolbar. Compute counts independent of the active filter so we
  // can hide them. Keep tabs visible while the manifest is still
  // loading (counts all zero) — otherwise the whole strip blanks
  // out for a beat on every modal open.
  const tabCounts = useMemo(() => {
    const c = { moby: 0, tie: 0, detail: 0, texture: 0, sound: 0, sky: 0 };
    if (manifest) {
      for (const e of manifest.entries) {
        if (e.kind === "moby") c.moby++;
        else if (e.kind === "tie") c.tie++;
        else if (e.kind === "detail") c.detail++;
        else if (e.kind === "texture") c.texture++;
        else if (e.kind === "sky") c.sky++;
      }
    }
    if (sounds) c.sound = sounds.length;
    return c;
  }, [manifest, sounds]);
  const manifestReady = manifest != null;
  const shouldShowTab = (kind: keyof typeof tabCounts) =>
    !manifestReady || tabCounts[kind] > 0;


  useEffect(() => {
    setSelectedTuid(null);
  }, [filter]);

  useEffect(() => {
    if (!open || !initialAssetTuid || !manifest) return;
    const entry = manifest.entries.find(
      (e) =>
        (e.kind === "moby" ||
          e.kind === "tie" ||
          e.kind === "detail" ||
          e.kind === "sky") &&
        e.tuid === initialAssetTuid,
    );
    if (entry) {
      setFilter(entry.kind as LibraryFilter);
      if (entry.kind !== "sky") {
        setSelectedTuid(initialAssetTuid);
      }
    }
  }, [open, initialAssetTuid, manifest]);

  useEffect(() => {
    if (!open || !initialPanel) return;
    setFilter(initialPanel);
  }, [open, initialPanel]);

  useEffect(() => {
    if (!open || !initialTextureId) return;
    setSelectedTextureId(initialTextureId);
  }, [open, initialTextureId]);

  useEffect(() => {
    if (!open || !initialSoundKey) return;
    setSelectedSoundKey(initialSoundKey);
  }, [open, initialSoundKey]);

  const [listCollapsed, setListCollapsed] = useState(false);
  const TEXTURE_PAGE_SIZE = 24;
  const SOUND_PAGE_SIZE = 18;
  const [texturePage, setTexturePage] = useState(0);
  const [soundPage, setSoundPage] = useState(0);
  useEffect(() => {
    setTexturePage(0);
  }, [search, filter]);
  useEffect(() => {
    setSoundPage(0);
  }, [search, filter]);
  const [fullscreen, setFullscreen] = useState(false);
  const [assetMulti, setAssetMulti] = useState<Set<string>>(new Set());
  const [textureMulti, setTextureMulti] = useState<Set<string>>(new Set());
  const [soundMulti, setSoundMulti] = useState<Set<string>>(new Set());
  const [bulkStatus, setBulkStatus] = useState<string | null>(null);
  const [bulkBusy, setBulkBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setAssetMulti(new Set());
      setTextureMulti(new Set());
      setSoundMulti(new Set());
      setBulkStatus(null);
    }
  }, [open]);
  useEffect(() => {
    setAssetMulti(new Set());
    setTextureMulti(new Set());
    setSoundMulti(new Set());
    setBulkStatus(null);
  }, [filter]);

  const handleRangeClick = useCallback(
    (
      kind: "asset" | "texture" | "sound",
      id: string,
      ids: string[],
      mods: { ctrl: boolean; shift: boolean },
    ) => {
      const setter =
        kind === "asset"
          ? setAssetMulti
          : kind === "texture"
            ? setTextureMulti
            : setSoundMulti;
      const lastIdRef =
        kind === "asset"
          ? lastClickedAssetRef
          : kind === "texture"
            ? lastClickedTextureRef
            : lastClickedSoundRef;
      if (mods.shift && lastIdRef.current) {
        const a = ids.indexOf(lastIdRef.current);
        const b = ids.indexOf(id);
        if (a >= 0 && b >= 0) {
          const [lo, hi] = a < b ? [a, b] : [b, a];
          const range = ids.slice(lo, hi + 1);
          setter((prev) => {
            const next = new Set(prev);
            for (const r of range) next.add(r);
            return next;
          });
        } else {
          setter(new Set([id]));
        }
      } else if (mods.ctrl) {
        setter((prev) => {
          const next = new Set(prev);
          if (next.has(id)) next.delete(id);
          else next.add(id);
          return next;
        });
        lastIdRef.current = id;
      } else {
        setter(new Set([id]));
        lastIdRef.current = id;
      }
    },
    [],
  );

  const lastClickedAssetRef = useRef<string | null>(null);
  const lastClickedTextureRef = useRef<string | null>(null);
  const lastClickedSoundRef = useRef<string | null>(null);
  useEffect(() => {
    if (!open) {
      setFullscreen(false);
    }
  }, [open]);
  useEffect(() => {
    if (!fullscreen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setFullscreen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [fullscreen]);

  const [selectedTextureId, setSelectedTextureId] = useState<string | null>(
    null,
  );
  const [textureBlobUrl, setTextureBlobUrl] = useState<string | null>(null);
  const [textureExportStatus, setTextureExportStatus] = useState<string | null>(
    null,
  );
  useEffect(() => {
    if (filter !== "texture") return;
    if (!folder || !selectedTextureId) {
      if (textureBlobUrl) URL.revokeObjectURL(textureBlobUrl);
      setTextureBlobUrl(null);
      return;
    }
    let cancelled = false;
    let url: string | null = null;
    const file = `textures/${selectedTextureId}.png`;
    readCachedBytes(folder, file)
      .then((bytes) => {
        if (cancelled) return;
        const blob = new Blob([bytes as ArrayBuffer], { type: "image/png" });
        url = URL.createObjectURL(blob);
        setTextureBlobUrl(url);
      })
      .catch(() => {
        if (cancelled) return;
        setTextureBlobUrl(null);
      });
    return () => {
      cancelled = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [filter, folder, selectedTextureId]);

  const [selectedSoundKey, setSelectedSoundKey] = useState<string | null>(null);

  // If the selected sound falls out of the current category filter, clear it.
  useEffect(() => {
    if (filter !== "sound" || !selectedSoundKey) return;
    if (!soundRows.some(
      (s) => `${s.source}-${s.index}-${s.name}` === selectedSoundKey,
    )) {
      setSelectedSoundKey(null);
    }
  }, [filter, soundRows, selectedSoundKey]);

  // Why: arrow-key navigation can step into a sound on a different
  // page than the one currently visible, which played the wrong-looking
  // track (active row is off-screen, the visible page still shows
  // unrelated rows). Whenever the selection changes, sync the page
  // forward/back to whichever page actually contains that row. Covers
  // arrow-key nav, modal-init via initialSoundKey, and any future
  // programmatic select.
  useEffect(() => {
    if (filter !== "sound" || !selectedSoundKey) return;
    const idx = soundRows.findIndex(
      (s) => `${s.source}-${s.index}-${s.name}` === selectedSoundKey,
    );
    if (idx < 0) return;
    const targetPage = Math.floor(idx / SOUND_PAGE_SIZE);
    if (targetPage !== soundPage) setSoundPage(targetPage);
  }, [filter, selectedSoundKey, soundRows, soundPage, SOUND_PAGE_SIZE]);

  const [decodedSoundCache, setDecodedSoundCache] = useState<
    Map<string, ExtractedSound>
  >(new Map());
  const [soundDecoding, setSoundDecoding] = useState(false);
  const [soundsError, setSoundsError] = useState<string | null>(null);
  useEffect(() => {
    if (!open) {
      setDecodedSoundCache(new Map());
      setSelectedSoundKey(null);
    }
  }, [open]);
  const selectedSoundEntry = useMemo<SoundEntry | null>(() => {
    if (!selectedSoundKey || !sounds) return null;
    return sounds.find(
      (s) => `${s.source}-${s.index}-${s.name}` === selectedSoundKey,
    ) ?? null;
  }, [selectedSoundKey, sounds]);
  const selectedExtractedSound: ExtractedSound | null = selectedSoundEntry
    ? decodedSoundCache.get(selectedSoundEntry.name) ?? null
    : null;
  useEffect(() => {
    if (!selectedSoundEntry || !folder) return;
    if (selectedSoundEntry.kind === "stream-missing") {
      setSoundsError(
        `Streaming sibling missing on disk for ${selectedSoundEntry.source}`,
      );
      return;
    }
    if (decodedSoundCache.has(selectedSoundEntry.name)) return;
    let cancelled = false;
    setSoundDecoding(true);
    setSoundsError(null);
    const isStream = selectedSoundEntry.kind === "stream";
    const promise = isStream
      ? extractOneStreamSound(
          folder,
          selectedSoundEntry.name,
          selectedSoundEntry.source,
        )
      : extractOneSound(
          folder,
          selectedSoundEntry.name,
          selectedSoundEntry.source,
        );
    promise
      .then((decoded) => {
        if (cancelled) return;
        setDecodedSoundCache((prev) => {
          const next = new Map(prev);
          next.set(decoded.name, decoded);
          return next;
        });
      })
      .catch((e) => {
        if (cancelled) return;
        setSoundsError(String(e));
      })
      .finally(() => {
        if (!cancelled) setSoundDecoding(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedSoundEntry, folder, decodedSoundCache]);
  const [nowPlaying, setNowPlaying] = useState<NowPlaying | null>(null);
  useEffect(() => {
    if (!selectedExtractedSound || !selectedSoundEntry) {
      setNowPlaying((prev) => {
        if (prev) {
          prev.audio.pause();
          URL.revokeObjectURL(prev.blobUrl);
        }
        return null;
      });
      return;
    }
    const wavBytes = Uint8Array.from(atob(selectedExtractedSound.wav_b64), (c) =>
      c.charCodeAt(0),
    );
    const blob = new Blob([wavBytes], { type: "audio/wav" });
    const url = URL.createObjectURL(blob);
    const audio = new Audio(url);
    audio.preload = "auto";
    audio.volume = 1;
    setNowPlaying((prev) => {
      if (prev) {
        prev.audio.pause();
        URL.revokeObjectURL(prev.blobUrl);
      }
      return {
        name: selectedSoundEntry.name,
        source: selectedSoundEntry.source,
        audio,
        blobUrl: url,
        entry: selectedExtractedSound,
      };
    });
    void audio.play().catch(() => {});
    return () => {
      audio.pause();
      URL.revokeObjectURL(url);
    };
  }, [selectedExtractedSound, selectedSoundEntry]);
  useEffect(() => {
    if (!open) {
      setNowPlaying((prev) => {
        if (prev) {
          prev.audio.pause();
          URL.revokeObjectURL(prev.blobUrl);
        }
        return null;
      });
    }
  }, [open]);

  const handleExportSound = useCallback(async () => {
    if (!selectedExtractedSound) return;
    try {
      const out = (await saveDialog({
        title: "Save sound as WAV",
        defaultPath: `${selectedExtractedSound.name}.wav`,
        filters: [{ name: "WAV audio", extensions: ["wav"] }],
      })) as string | null;
      if (!out) return;
      const wavBytes = Uint8Array.from(
        atob(selectedExtractedSound.wav_b64),
        (c) => c.charCodeAt(0),
      );
      await writeBytes(out, Array.from(wavBytes));
    } catch (e) {
      console.error("sound export failed", e);
    }
  }, [selectedExtractedSound]);

  const handleBulkExportAssets = useCallback(async () => {
    if (!folder || assetMulti.size === 0 || !manifest) return;
    setBulkBusy(true);
    setBulkStatus(`Picking output folder…`);
    try {
      const out = (await openDialog({
        directory: true,
        title: `Choose output folder for ${assetMulti.size} assets`,
      })) as string | null;
      if (!out) {
        setBulkStatus(null);
        setBulkBusy(false);
        return;
      }
      let ok = 0;
      let failed = 0;
      const ids = Array.from(assetMulti);
      for (let i = 0; i < ids.length; i++) {
        const tuid = ids[i]!;
        setBulkStatus(`Exporting ${i + 1}/${ids.length}…`);
        try {
          await invoke<number>("export_cached_moby_glb", {
            levelFolder: folder,
            assetTuidHex: tuid,
            outPath: `${out}/${tuid}.glb`,
          });
          ok++;
        } catch {
          failed++;
        }
      }
      setBulkStatus(
        `Exported ${ok} GLBs to ${out}${failed > 0 ? ` (${failed} failed)` : ""}`,
      );
    } finally {
      setBulkBusy(false);
    }
  }, [folder, assetMulti, manifest]);

  const handleBulkExportTextures = useCallback(
    async (format: "png" | "dds") => {
      if (!folder || textureMulti.size === 0) return;
      setBulkBusy(true);
      setBulkStatus("Picking output folder…");
      try {
        const out = (await openDialog({
          directory: true,
          title: `Choose output folder for ${textureMulti.size} textures`,
        })) as string | null;
        if (!out) {
          setBulkStatus(null);
          setBulkBusy(false);
          return;
        }
        let ok = 0;
        let failed = 0;
        const ids = Array.from(textureMulti);
        for (let i = 0; i < ids.length; i++) {
          const idStr = ids[i]!;
          const texId = parseInt(idStr, 10);
          if (!Number.isFinite(texId)) {
            failed++;
            continue;
          }
          setBulkStatus(`Exporting ${i + 1}/${ids.length}…`);
          try {
            const outPath = `${out}/${texId}.${format}`;
            if (format === "png") {
              await exportTexturePng(folder, texId, outPath);
            } else {
              await exportTextureDds(folder, texId, outPath);
            }
            ok++;
          } catch {
            failed++;
          }
        }
        setBulkStatus(
          `Exported ${ok} ${format.toUpperCase()}s to ${out}${failed > 0 ? ` (${failed} failed)` : ""}`,
        );
      } finally {
        setBulkBusy(false);
      }
    },
    [folder, textureMulti],
  );

  const handleBulkExportSounds = useCallback(async () => {
    if (!folder || soundMulti.size === 0 || !sounds) return;
    setBulkBusy(true);
    setBulkStatus("Picking output folder…");
    try {
      const out = (await openDialog({
        directory: true,
        title: `Choose output folder for ${soundMulti.size} sounds`,
      })) as string | null;
      if (!out) {
        setBulkStatus(null);
        setBulkBusy(false);
        return;
      }
      let ok = 0;
      let failed = 0;
      const keys = Array.from(soundMulti);
      for (let i = 0; i < keys.length; i++) {
        const k = keys[i]!;
        const entry = sounds.find(
          (s) => `${s.source}-${s.index}-${s.name}` === k,
        );
        if (!entry || entry.kind !== "bank") {
          failed++;
          continue;
        }
        setBulkStatus(`Exporting ${i + 1}/${keys.length}…`);
        try {
          const decoded =
            decodedSoundCache.get(entry.name) ??
            (await extractOneSound(folder, entry.name, entry.source));
          if (!decodedSoundCache.has(entry.name)) {
            setDecodedSoundCache((prev) => {
              const next = new Map(prev);
              next.set(decoded.name, decoded);
              return next;
            });
          }
          const wavBytes = Uint8Array.from(atob(decoded.wav_b64), (c) =>
            c.charCodeAt(0),
          );
          const safeName = entry.name.replace(/[\\/:*?"<>|]/g, "_");
          await writeBytes(`${out}/${safeName}.wav`, Array.from(wavBytes));
          ok++;
        } catch {
          failed++;
        }
      }
      setBulkStatus(
        `Exported ${ok} WAVs to ${out}${failed > 0 ? ` (${failed} failed/non-bank)` : ""}`,
      );
    } finally {
      setBulkBusy(false);
    }
  }, [folder, soundMulti, sounds, decodedSoundCache]);

  /// Pack every bank + stream sound in the level into a single .zip.
  /// Doesn't honor the multi-select set — this is the "give me
  /// everything" action. Multi-select still uses the existing
  /// folder-output flow above so users can pick subsets when they want.
  const handleBulkExportSoundsZip = useCallback(async () => {
    if (!folder) return;
    setBulkBusy(true);
    setBulkStatus("Picking output zip…");
    try {
      const out = (await saveDialog({
        title: "Save all sounds as .zip",
        defaultPath: "sounds.zip",
        filters: [{ name: "Zip archive", extensions: ["zip"] }],
      })) as string | null;
      if (!out) {
        setBulkStatus(null);
        return;
      }
      setBulkStatus("Packing all sounds into zip…");
      const written = await bulkExtractSoundsZip(folder, out);
      setBulkStatus(`Wrote ${written.toLocaleString()} WAVs to ${out}`);
    } catch (e) {
      setBulkStatus(`Zip export failed: ${String(e)}`);
    } finally {
      setBulkBusy(false);
    }
  }, [folder]);

  const handleExportTexture = useCallback(
    async (format: "png" | "dds") => {
      if (!folder || !selectedTextureId) return;
      const texId = parseInt(selectedTextureId, 10);
      if (!Number.isFinite(texId)) return;
      try {
        const out = (await saveDialog({
          title: `Save texture as ${format.toUpperCase()}`,
          defaultPath: `${texId}.${format}`,
          filters: [
            format === "png"
              ? { name: "PNG image", extensions: ["png"] }
              : { name: "DirectDraw Surface", extensions: ["dds"] },
          ],
        })) as string | null;
        if (!out) return;
        if (format === "png") {
          const bytes = await exportTexturePng(folder, texId, out);
          setTextureExportStatus(`Saved ${(bytes / 1024).toFixed(0)} KB`);
        } else {
          const bytes = await exportTextureDds(folder, texId, out);
          setTextureExportStatus(`Saved ${(bytes / 1024).toFixed(0)} KB DDS`);
        }
      } catch (e) {
        setTextureExportStatus(`Export failed: ${String(e)}`);
      }
    },
    [folder, selectedTextureId],
  );

  const totalShown = useMemo(
    () => grouped.reduce((sum, b) => sum + b.rows.length, 0),
    [grouped],
  );

  const flatRows = useMemo(
    () => grouped.flatMap((b) => b.rows),
    [grouped],
  );

  const rowRefs = useRef(new Map<string, HTMLButtonElement>());
  const setRowRef = useCallback(
    (tuid: string) => (el: HTMLButtonElement | null) => {
      if (el) rowRefs.current.set(tuid, el);
      else rowRefs.current.delete(tuid);
    },
    [],
  );

  const moveSelection = useCallback(
    (direction: 1 | -1) => {
      if (filter === "texture") {
        if (textureRows.length === 0) return;
        const currentIndex = selectedTextureId
          ? textureRows.findIndex((r) => r.tuid === selectedTextureId)
          : -1;
        const nextIndex =
          currentIndex === -1
            ? direction === 1
              ? 0
              : textureRows.length - 1
            : Math.max(
                0,
                Math.min(textureRows.length - 1, currentIndex + direction),
              );
        const nextId = textureRows[nextIndex]?.tuid;
        if (nextId && nextId !== selectedTextureId) {
          setSelectedTextureId(nextId);
        }
        return;
      }
      if (filter === "sound") {
        if (soundRows.length === 0) return;
        const keyOf = (s: SoundEntry) => `${s.source}-${s.index}-${s.name}`;
        const currentIndex = selectedSoundKey
          ? soundRows.findIndex((s) => keyOf(s) === selectedSoundKey)
          : -1;
        const nextIndex =
          currentIndex === -1
            ? direction === 1
              ? 0
              : soundRows.length - 1
            : Math.max(
                0,
                Math.min(soundRows.length - 1, currentIndex + direction),
              );
        const next = soundRows[nextIndex];
        if (next) {
          const k = keyOf(next);
          if (k !== selectedSoundKey) setSelectedSoundKey(k);
        }
        return;
      }
      if (flatRows.length === 0) return;
      const currentIndex = selectedTuid
        ? flatRows.findIndex((r) => r.entry.tuid === selectedTuid)
        : -1;
      let nextIndex: number;
      if (currentIndex === -1) {
        nextIndex = direction === 1 ? 0 : flatRows.length - 1;
      } else {
        nextIndex = Math.max(
          0,
          Math.min(flatRows.length - 1, currentIndex + direction),
        );
      }
      const nextTuid = flatRows[nextIndex]?.entry.tuid;
      if (nextTuid && nextTuid !== selectedTuid) {
        setSelectedTuid(nextTuid);
      }
    },
    [
      filter,
      flatRows,
      textureRows,
      soundRows,
      selectedTuid,
      selectedTextureId,
      selectedSoundKey,
    ],
  );

  
  
  
  
  const previewInstance: Instance | null =
    selectedAsset &&
    (filter === "moby" || filter === "tie" || filter === "detail")
      ? {
          tuid: `${selectedAsset.asset_tuid}#cache`,
          asset_tuid: selectedAsset.asset_tuid,
          kind: filter,
          name: selectedAsset.name || selectedAsset.asset_tuid,
          position: [0, 0, 0],
          quaternion: [0, 0, 0, 1],
          scale: [1, 1, 1],
        }
      : null;

  
  
  
  
  const previewMeshes: LevelMeshes | null = selectedAsset
    ? {
        moby_assets: filter === "moby" ? [selectedAsset] : [],
        tie_assets: filter === "tie" ? [selectedAsset] : [],
        detail_assets: filter === "detail" ? [selectedAsset] : [],
        ufrag_meshes: [],
        textures: [...selectedTextures.keys()].map((id) => ({
          id,
          width: 0,
          height: 0,
        })),
      }
    : null;

  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const [exportModalOpen, setExportModalOpen] = useState(false);
  const [previewPicks, setPreviewPicks] = useState<ExportPicks>({ byAnimset: {} });

  useEffect(() => {
    setPreviewPicks({ byAnimset: {} });
  }, [selectedTuid]);

  useEffect(() => {
    if (!selectedTuid) return;
    const el = rowRefs.current.get(selectedTuid);
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedTuid]);

  useEffect(() => {
    if (!open || exportModalOpen) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
      const target = event.target as HTMLElement | null;
      if (target) {
        const tag = target.tagName;
        if (
          tag === "INPUT" ||
          tag === "TEXTAREA" ||
          tag === "SELECT" ||
          target.isContentEditable
        ) {
          return;
        }
      }
      event.preventDefault();
      moveSelection(event.key === "ArrowDown" ? 1 : -1);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [open, exportModalOpen, moveSelection]);
  const handleExport = () => {
    if (!selectedAsset || exporting || !folder) return;
    setExportStatus(null);
    setExportModalOpen(true);
  };

  
  
  
  const handleReextract = async () => {
    if (!folder || reextractStatus) return;
    setReextractStatus("Re-extracting…");
    setSelectedTuid(null);
    setSelectedAsset(null);
    setManifest(null);
    const channel = new Channel<CacheEvent>();
    let phase: "mobys" | "ties" | "materials" | "normalmaps" | "textures" | "ufrags" = "mobys";
    channel.onmessage = (event) => {
      switch (event.type) {
        case "phase":
          phase = event.phase;
          setReextractStatus(`Re-extracting ${phase} 0/${event.total}`);
          break;
        case "progress":
          setReextractStatus((s) =>
            s ? s.replace(/\d+\//, `${event.current}/`) : s,
          );
          break;
        case "done":
          setReextractStatus(null);
          readCachedManifest(folder)
            .then(setManifest)
            .catch((e) => setManifestError(String(e)));
          break;
        case "error":
          setReextractStatus(null);
          setManifestError(event.message);
          break;
      }
    };
    try {
      await reextractLevelCache(folder, channel);
    } catch (e) {
      setReextractStatus(null);
      setManifestError(String(e));
    }
  };

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={
        <span className="cache-library-title">
          <Database size={16} /> Cache Library
        </span>
      }
      subtitle={
        manifest
          ? `${manifest.entries.length} entries · ${manifest.folder}`
          : folder ?? "No level open"
      }
      subheader={
        <div className="cache-library-subheader">
          <div className="cache-library-tabs cache-library-tabs--full" role="tablist">
            {shouldShowTab("moby") && (
              <button
                type="button"
                role="tab"
                aria-selected={filter === "moby"}
                className={`cache-library-tab ${filter === "moby" ? "active" : ""}`}
                onClick={() => setFilter("moby")}
              >
                Mobys
              </button>
            )}
            {shouldShowTab("tie") && (
              <button
                type="button"
                role="tab"
                aria-selected={filter === "tie"}
                className={`cache-library-tab ${filter === "tie" ? "active" : ""}`}
                onClick={() => setFilter("tie")}
              >
                Ties
              </button>
            )}
            {shouldShowTab("detail") && (
              <button
                type="button"
                role="tab"
                aria-selected={filter === "detail"}
                className={`cache-library-tab cache-library-tab--detail ${filter === "detail" ? "active" : ""}`}
                onClick={() => setFilter("detail")}
              >
                Details
              </button>
            )}
            {shouldShowTab("texture") && (
              <button
                type="button"
                role="tab"
                aria-selected={filter === "texture"}
                className={`cache-library-tab ${filter === "texture" ? "active" : ""}`}
                onClick={() => setFilter("texture")}
              >
                Textures
              </button>
            )}
            {shouldShowTab("sound") && (
              <button
                type="button"
                role="tab"
                aria-selected={filter === "sound"}
                className={`cache-library-tab ${filter === "sound" ? "active" : ""}`}
                onClick={() => setFilter("sound")}
              >
                Sounds
              </button>
            )}
            {shouldShowTab("sky") && (
              <button
                type="button"
                role="tab"
                aria-selected={filter === "sky"}
                className={`cache-library-tab cache-library-tab--sky ${filter === "sky" ? "active" : ""}`}
                onClick={() => setFilter("sky")}
              >
                Cubemap
              </button>
            )}
          </div>
          <button
            type="button"
            className="cache-library-reextract"
            onClick={handleReextract}
            disabled={!folder || reextractStatus !== null}
            title="Wipe and rebuild the cache from source .dat files"
          >
            <RefreshCw size={12} />
            Re-extract
          </button>
        </div>
      }
      size="xl"
      bodyClassName="cache-library-modal-body"
      footer={
        <>
          {bulkStatus && (
            <span className="dim small" style={{ marginRight: "auto" }}>
              {bulkStatus}
            </span>
          )}
          <Button onClick={onClose}>Close</Button>
          {(filter === "moby" || filter === "tie" || filter === "detail") &&
            (assetMulti.size > 1 ? (
              <Button
                variant="primary"
                icon={Download}
                onClick={() => void handleBulkExportAssets()}
                disabled={!folder || bulkBusy}
                loading={bulkBusy}
              >
                {bulkBusy ? "Exporting…" : `Export ${assetMulti.size} GLBs`}
              </Button>
            ) : (
              <Button
                variant="primary"
                icon={Download}
                onClick={() => handleExport()}
                disabled={!selectedAsset}
                loading={exporting}
              >
                {exporting ? "Exporting…" : "Export .glb"}
              </Button>
            ))}
          {filter === "texture" &&
            (textureMulti.size > 1 ? (
              <>
                <Button
                  icon={Download}
                  onClick={() => void handleBulkExportTextures("dds")}
                  disabled={!folder || bulkBusy}
                  loading={bulkBusy}
                >
                  {bulkBusy ? "Exporting…" : `Save ${textureMulti.size} DDS`}
                </Button>
                <Button
                  variant="primary"
                  icon={Download}
                  onClick={() => void handleBulkExportTextures("png")}
                  disabled={!folder || bulkBusy}
                  loading={bulkBusy}
                >
                  {bulkBusy ? "Exporting…" : `Save ${textureMulti.size} PNGs`}
                </Button>
              </>
            ) : (
              <>
                <Button
                  icon={Download}
                  onClick={() => void handleExportTexture("dds")}
                  disabled={!selectedTextureId || !folder}
                >
                  Save DDS
                </Button>
                <Button
                  variant="primary"
                  icon={Download}
                  onClick={() => void handleExportTexture("png")}
                  disabled={!selectedTextureId || !folder}
                >
                  Save PNG
                </Button>
                {onUseAsSkybox && (
                  <Button
                    onClick={() => {
                      const id = selectedTextureId
                        ? parseInt(selectedTextureId, 10)
                        : NaN;
                      if (Number.isFinite(id)) onUseAsSkybox(id);
                    }}
                    disabled={!selectedTextureId}
                  >
                    Use as cubemap
                  </Button>
                )}
              </>
            ))}
          {filter === "sound" && (
            <>
              <Button
                icon={Download}
                onClick={() => void handleBulkExportSoundsZip()}
                disabled={!folder || bulkBusy}
                loading={bulkBusy}
                title="Pack every sound + music + dialog stream into a single .zip"
              >
                {bulkBusy ? "Packing…" : "Extract all → .zip"}
              </Button>
              {soundMulti.size > 1 ? (
                <Button
                  variant="primary"
                  icon={Download}
                  onClick={() => void handleBulkExportSounds()}
                  disabled={!folder || bulkBusy}
                  loading={bulkBusy}
                >
                  {bulkBusy ? "Exporting…" : `Save ${soundMulti.size} WAVs`}
                </Button>
              ) : (
                <Button
                  variant="primary"
                  icon={Download}
                  onClick={() => void handleExportSound()}
                  disabled={!selectedExtractedSound}
                >
                  Save WAV
                </Button>
              )}
            </>
          )}
        </>
      }
    >
      <div
        className={`cache-library-body${
          filter === "sound" ? " is-playlist" : ""
        }${filter === "texture" ? " is-gallery" : ""}${
          listCollapsed ? " is-list-collapsed" : ""
        }`}
      >
        <div className="cache-library-list">
          {(filter === "moby" || filter === "tie" || filter === "detail") && (
            <div className="cache-library-toolbar">
              <button
                type="button"
                className="cache-library-collapse-btn"
                onClick={() => setListCollapsed((p) => !p)}
                title={listCollapsed ? "Show list" : "Hide list"}
                aria-label={listCollapsed ? "Show list" : "Hide list"}
              >
                {listCollapsed ? (
                  <ChevronRight size={14} />
                ) : (
                  <ChevronLeft size={14} />
                )}
              </button>
            </div>
          )}
          {reextractStatus && (
            <div className="dim small" style={{ padding: "4px 10px" }}>
              {reextractStatus}
            </div>
          )}
          <div className="cache-library-search">
            <Search size={13} />
            <input
              type="search"
              placeholder="Search by name or TUID"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              spellCheck={false}
            />
            <span className="dim small">{totalShown}</span>
          </div>
          {filter === "sound" && (
            <div className="cache-library-sound-toolbar">
              <div
                className="cache-library-subtabs"
                role="tablist"
                aria-label="Sound category"
              >
                {(
                  [
                    ["all", "All"],
                    ["sfx", "SFX"],
                    ["dialog", "Dialog"],
                    ["music", "Music"],
                  ] as const
                ).map(([key, label]) => {
                  const count = soundCategoryCounts[key];
                  const isActive = soundCategory === key;
                  return (
                    <button
                      key={key}
                      type="button"
                      role="tab"
                      aria-selected={isActive}
                      className={`cache-library-subtab${
                        isActive ? " active" : ""
                      }`}
                      onClick={() => setSoundCategory(key)}
                    >
                      {label}
                      <span className="cache-library-subtab-count dim small">
                        {count}
                      </span>
                    </button>
                  );
                })}
              </div>
              <Button
                onClick={handleBulkSoundZip}
                disabled={!folder || bulkSoundZipBusy}
                title="Decode every sound bank + stream and pack the WAVs into a single .zip"
              >
                <Download size={14} />
                {bulkSoundZipBusy ? "Packing…" : "Download all as ZIP"}
              </Button>
            </div>
          )}
          {filter === "sound" && bulkSoundZipStatus && (
            <div className="small dim" style={{ padding: "4px 12px" }}>
              {bulkSoundZipStatus}
            </div>
          )}
          {manifestError && (
            <div className="cache-library-extract-cta">
              <div className="cache-library-extract-title">No cache yet</div>
              <div className="cache-library-extract-hint small dim">
                {manifestError}
              </div>
              {onRequestExtract && (
                <Button
                  variant="primary"
                  icon={Download}
                  onClick={onRequestExtract}
                  disabled={!folder}
                >
                  Extract level to cache
                </Button>
              )}
            </div>
          )}
          {!manifest && !manifestError && (
            <div className="dim small" style={{ padding: 12 }}>
              Loading manifest…
            </div>
          )}
          {manifest &&
            initialAssetTuid &&
            !manifest.entries.some(
              (e) =>
                (e.kind === "moby" ||
                  e.kind === "tie" ||
                  e.kind === "detail" ||
                  e.kind === "sky") &&
                e.tuid === initialAssetTuid,
            ) && (
              <div className="cache-library-extract-cta">
                <div className="cache-library-extract-title">
                  Asset not in cache
                </div>
                <div className="cache-library-extract-hint small dim">
                  Extract the level to populate this asset.
                </div>
                {onRequestExtract && (
                  <Button
                    variant="primary"
                    icon={Download}
                    onClick={onRequestExtract}
                    disabled={!folder}
                  >
                    Extract level to cache
                  </Button>
                )}
              </div>
            )}
          {manifest &&
            totalShown === 0 &&
            (filter === "moby" ||
              filter === "tie" ||
              filter === "detail") && (
              <div className="dim small" style={{ padding: 12 }}>
                No {filter === "moby" ? "mobys" : filter === "tie" ? "ties" : "details"} match this search.
              </div>
            )}
          {(filter === "moby" || filter === "tie" || filter === "detail") && (
            <ul className="cache-library-rows">
              {grouped.map((bucket) => (
                <li key={bucket.group} className="cache-library-bucket">
                  <div className="cache-library-group">{bucket.group}</div>
                  {bucket.rows.map((row) => {
                    const active = row.entry.tuid === selectedTuid;
                    const multi = assetMulti.has(row.entry.tuid);
                    return (
                      <button
                        key={row.entry.tuid}
                        ref={setRowRef(row.entry.tuid)}
                        type="button"
                        className={`cache-library-row ${active ? "active" : ""}${multi ? " is-multi" : ""}`}
                        onClick={(e) => {
                          const mods = {
                            ctrl: e.ctrlKey || e.metaKey,
                            shift: e.shiftKey,
                          };
                          if (mods.ctrl || mods.shift) {
                            handleRangeClick(
                              "asset",
                              row.entry.tuid,
                              flatRows.map((r) => r.entry.tuid),
                              mods,
                            );
                          } else {
                            setSelectedTuid(row.entry.tuid);
                            setAssetMulti(new Set([row.entry.tuid]));
                            lastClickedAssetRef.current = row.entry.tuid;
                          }
                        }}
                      >
                        <span className="cache-library-leaf">{row.leaf}</span>
                        <span className="cache-library-tuid mono">
                          {row.entry.tuid.slice(-8)}
                        </span>
                      </button>
                    );
                  })}
                </li>
              ))}
            </ul>
          )}

          {filter === "texture" && (() => {
            const totalPages = Math.max(
              1,
              Math.ceil(textureRows.length / TEXTURE_PAGE_SIZE),
            );
            const safePage = Math.min(texturePage, totalPages - 1);
            const start = safePage * TEXTURE_PAGE_SIZE;
            const visible = textureRows.slice(start, start + TEXTURE_PAGE_SIZE);
            return (
              <div className="cache-texture-gallery">
                {textureRows.length === 0 && manifest && (
                  <div className="dim small" style={{ padding: 12 }}>
                    No textures match this search.
                  </div>
                )}
                <div className="cache-texture-gallery-grid">
                  {visible.map((entry) => {
                    const active = entry.tuid === selectedTextureId;
                    const multi = textureMulti.has(entry.tuid);
                    return (
                      <TextureTile
                        key={entry.tuid}
                        folder={folder}
                        entry={entry}
                        active={active}
                        multi={multi}
                        onClick={(mods) => {
                          if (mods.ctrl || mods.shift) {
                            handleRangeClick(
                              "texture",
                              entry.tuid,
                              textureRows.map((r) => r.tuid),
                              mods,
                            );
                          } else {
                            setSelectedTextureId(entry.tuid);
                            setTextureMulti(new Set([entry.tuid]));
                            lastClickedTextureRef.current = entry.tuid;
                          }
                        }}
                      />
                    );
                  })}
                </div>
                {textureRows.length > TEXTURE_PAGE_SIZE && (
                  <Paginator
                    currentPage={safePage}
                    totalPages={totalPages}
                    onChange={setTexturePage}
                  />
                )}
              </div>
            );
          })()}

          {filter === "sound" && (() => {
            const soundTotalPages = Math.max(
              1,
              Math.ceil(soundRows.length / SOUND_PAGE_SIZE),
            );
            const soundSafePage = Math.min(soundPage, soundTotalPages - 1);
            const soundStart = soundSafePage * SOUND_PAGE_SIZE;
            const visibleSounds = soundRows.slice(
              soundStart,
              soundStart + SOUND_PAGE_SIZE,
            );
            return (
            <div className="cache-sound-playlist">
              <ul className="cache-sound-playlist-rows">
                {soundRows.length === 0 && (
                  <li className="dim small" style={{ padding: 12 }}>
                    {sounds === undefined
                      ? "No sounds loaded yet."
                      : "No sounds match this search."}
                  </li>
                )}
                {visibleSounds.map((s, idxInPage) => {
                  const listIdx = soundStart + idxInPage;
                  const key = `${s.source}-${s.index}-${s.name}`;
                  const active = key === selectedSoundKey;
                  const multi = soundMulti.has(key);
                  const cached = decodedSoundCache.has(s.name);
                  const isLoading = active && soundDecoding && !cached;
                  const isPlayingHere =
                    active && nowPlaying && !nowPlaying.audio.paused;
                  return (
                    <button
                      key={key}
                      ref={(el) => {
                        if (active && el)
                          el.scrollIntoView({ block: "nearest" });
                      }}
                      type="button"
                      className={`cache-sound-row${active ? " is-active" : ""}${isPlayingHere ? " is-playing" : ""}${multi ? " is-multi" : ""}`}
                      onClick={(e) => {
                        const mods = {
                          ctrl: e.ctrlKey || e.metaKey,
                          shift: e.shiftKey,
                        };
                        if (mods.ctrl || mods.shift) {
                          handleRangeClick(
                            "sound",
                            key,
                            soundRows.map(
                              (r) => `${r.source}-${r.index}-${r.name}`,
                            ),
                            mods,
                          );
                        } else {
                          setSelectedSoundKey(key);
                          setSoundMulti(new Set([key]));
                          lastClickedSoundRef.current = key;
                        }
                      }}
                      title={`${s.kind} · ${s.source}`}
                    >
                      <span className="cache-sound-row-index mono">
                        {listIdx + 1}
                      </span>
                      <span
                        className="cache-sound-row-icon"
                        role="button"
                        title={isPlayingHere ? "Pause" : "Play"}
                        onClick={(e) => {
                          if (isPlayingHere && nowPlaying) {
                            e.stopPropagation();
                            nowPlaying.audio.pause();
                          } else if (active && nowPlaying) {
                            e.stopPropagation();
                            void nowPlaying.audio.play().catch(() => {});
                          }
                        }}
                      >
                        {isLoading ? (
                          <span className="cache-sound-row-spinner" />
                        ) : isPlayingHere ? (
                          "❚❚"
                        ) : (
                          "▶"
                        )}
                      </span>
                      <span className="cache-sound-row-meta">
                        <span className="cache-sound-row-name">{s.name}</span>
                        <span className="cache-sound-row-source dim small mono">
                          {s.kind} · {s.source} · #{s.index}
                        </span>
                      </span>
                      {cached && (
                        <span
                          className="cache-sound-row-cached small dim"
                          title="Decoded WAV is cached"
                        >
                          ✓
                        </span>
                      )}
                    </button>
                  );
                })}
              </ul>
              <div className="cache-sound-playlist-dock">
                {soundDecoding && !nowPlaying && (
                  <div className="cache-sound-loading">
                    <span className="cache-sound-loading-spinner" aria-hidden />
                    <span className="dim small">
                      Decoding bank — first click on this level may take a few
                      seconds…
                    </span>
                  </div>
                )}
                {soundsError && !soundDecoding && (
                  <div className="cache-sound-error">
                    <strong className="small">Sound not playable yet</strong>
                    <pre
                      className="mono small"
                      style={{
                        marginTop: 6,
                        whiteSpace: "pre-wrap",
                        color: "var(--text-3)",
                      }}
                    >
                      {soundsError}
                    </pre>
                  </div>
                )}
                {/* Player is always mounted now — placeholder when idle,
                    active when something is selected. Volume is shared
                    across every track via localStorage. */}
                <SoundPlayer nowPlaying={nowPlaying} />
                {!soundDecoding &&
                  !nowPlaying &&
                  !soundsError &&
                  selectedSoundEntry?.kind === "stream-missing" && (
                    <div className="dim small" style={{ padding: 12 }}>
                      Streaming sibling not on disk — extract the level's
                      streaming files first.
                    </div>
                  )}
              </div>
              {soundRows.length > SOUND_PAGE_SIZE && (
                <Paginator
                  currentPage={soundSafePage}
                  totalPages={soundTotalPages}
                  onChange={setSoundPage}
                />
              )}
            </div>
            );
          })()}
        </div>
        <div className="cache-library-preview">
          {(filter === "moby" || filter === "tie" || filter === "detail") &&
            !selectedAsset &&
            !loadingAsset && (
              <div className="cache-library-empty dim">
                Select a {filter === "moby" ? "moby" : filter === "tie" ? "tie" : "detail cluster"} on the left to preview it.
              </div>
            )}
          {(filter === "moby" || filter === "tie" || filter === "detail") &&
            loadingAsset && (
              <div className="cache-library-empty dim">Loading asset…</div>
            )}

          {filter === "texture" && !selectedTextureId && (
            <div className="cache-library-empty dim">
              Select a texture on the left to preview it.
            </div>
          )}
          {filter === "texture" && selectedTextureId && (
            <div className="cache-texture-preview">
              <div className="cache-texture-preview-frame">
                {textureBlobUrl ? (
                  <img
                    src={textureBlobUrl}
                    alt={`texture ${selectedTextureId}`}
                    className="cache-texture-preview-img"
                  />
                ) : (
                  <span className="dim small">Loading…</span>
                )}
              </div>
              <div className="cache-texture-preview-actions">
                <span className="mono small">id {selectedTextureId}</span>
                {textureExportStatus && (
                  <span className="dim small">{textureExportStatus}</span>
                )}
              </div>
            </div>
          )}

          {filter === "sky" && (
            <SkyTexturePanel
              folder={folder}
              currentSkyboxTextureId={currentSkyboxTextureId ?? null}
              onPickTexture={() => setFilter("texture")}
              onClearSkybox={() => onUseAsSkybox?.(-1)}
              onExportPng={async () => {
                if (currentSkyboxTextureId == null || !folder) return;
                const out = (await saveDialog({
                  defaultPath: `skybox_${currentSkyboxTextureId}.png`,
                  filters: [{ name: "PNG", extensions: ["png"] }],
                })) as string | null;
                if (out) await exportTexturePng(folder, currentSkyboxTextureId, out);
              }}
              onExportDds={async () => {
                if (currentSkyboxTextureId == null || !folder) return;
                const out = (await saveDialog({
                  defaultPath: `skybox_${currentSkyboxTextureId}.dds`,
                  filters: [{ name: "DDS", extensions: ["dds"] }],
                })) as string | null;
                if (out) await exportTextureDds(folder, currentSkyboxTextureId, out);
              }}
            />
          )}

          {(filter === "moby" || filter === "tie" || filter === "detail") &&
            selectedAsset && (
            <>
              {(() => {
                const idx = selectedTuid
                  ? flatRows.findIndex((r) => r.entry.tuid === selectedTuid)
                  : -1;
                const total = flatRows.length;
                const canPrev = idx > 0;
                const canNext = idx >= 0 && idx < total - 1;
                return (
                  <div className="cache-library-navbar">
                    <button
                      type="button"
                      className="btn small cache-library-nav-btn"
                      onClick={() => moveSelection(-1)}
                      disabled={!canPrev}
                      title="Previous (↑)"
                      aria-label="Previous asset"
                    >
                      ↑
                    </button>
                    <button
                      type="button"
                      className="btn small cache-library-nav-btn"
                      onClick={() => moveSelection(1)}
                      disabled={!canNext}
                      title="Next (↓)"
                      aria-label="Next asset"
                    >
                      ↓
                    </button>
                    <span className="dim small cache-library-nav-pos">
                      {idx >= 0 ? `${idx + 1} / ${total}` : `– / ${total}`}
                    </span>
                    <button
                      type="button"
                      className="btn small cache-library-nav-btn"
                      onClick={() => {
                        // Why: user prefers the layered Asset workbench
                        // tab over the in-modal fullscreen — full submesh
                        // list, texture thumbnails, and animation
                        // scrubber. Falls back to the old fullscreen
                        // behavior if the host didn't pass the callback.
                        if (
                          onOpenInWorkbench &&
                          (filter === "moby" || filter === "tie") &&
                          selectedAsset
                        ) {
                          const tuid = selectedAsset.asset_tuid
                            .split("#")[0];
                          if (tuid) {
                            onOpenInWorkbench(tuid, filter);
                            return;
                          }
                        }
                        setFullscreen(true);
                      }}
                      title="Open in Asset workbench"
                      aria-label="Open in Asset workbench"
                    >
                      <Maximize2 size={12} />
                    </button>
                  </div>
                );
              })()}
              <div className="cache-library-canvas">
                <AssetPreview
                  instance={previewInstance}
                  meshes={previewMeshes}
                  textureBlobs={selectedTextures.size > 0 ? selectedTextures : null}
                  cacheFolder={folder ?? undefined}
                  exportPicks={previewPicks}
                  onExportPicksChange={setPreviewPicks}
                />
              </div>
              <dl className="kv cache-library-meta">
                <dt>Name</dt>
                <dd className="mono small">
                  {selectedAsset.name || (
                    <span className="dim">unnamed</span>
                  )}
                </dd>
                <dt>Asset</dt>
                <dd className="mono small">{selectedAsset.asset_tuid}</dd>
                <dt>Submeshes</dt>
                <dd>{selectedAsset.submeshes.length}</dd>
                <dt>Skeleton</dt>
                <dd>
                  {selectedAsset.skeleton
                    ? `${selectedAsset.skeleton.bone_count} bones`
                    : "none"}
                </dd>
                <dt>Animset</dt>
                <dd className="mono small">
                  {selectedAsset.animset_hash ? (
                    selectedAsset.animset_hash
                  ) : (selectedAsset.embedded_animation_count ?? 0) > 0 ? (
                    <span>
                      embedded ({selectedAsset.embedded_animation_count} clips)
                    </span>
                  ) : (
                    <span className="dim">none</span>
                  )}
                </dd>
              </dl>
              <p className="dim small" style={{ marginTop: 8 }}>
                {selectedTextures.size > 0
                  ? `Loaded ${selectedTextures.size} textures from cache. `
                  : "No textures referenced. "}
                The .glb export copies the pre-baked file from
                <code> _rechimera_cache/mobys/</code> — geometry +
                skeleton + animations + textures all embedded.
              </p>
              {exportStatus && (
                <p
                  className="small"
                  style={{
                    marginTop: 4,
                    color: exportStatus.startsWith("Export failed")
                      ? "var(--accent-yellow)"
                      : "var(--text-2)",
                  }}
                >
                  {exportStatus}
                </p>
              )}
            </>
          )}
        </div>
      </div>
      {selectedAsset && folder && (
        <ExportOptionsModal
          open={exportModalOpen}
          folder={folder}
          assetTuidHex={selectedAsset.asset_tuid}
          assetName={selectedAsset.name}
          hasSkeleton={selectedAsset.skeleton != null}
          primaryAnimsetHash={selectedAsset.animset_hash ?? null}
          initialExtraPicks={previewPicks.byAnimset}
          onClose={() => setExportModalOpen(false)}
          onExported={(path, bytes) => {
            setExportStatus(`Exported ${bytes.toLocaleString()} bytes → ${path}`);
          }}
        />
      )}

      {fullscreen && (
        <div className="cache-fullscreen" role="dialog" aria-modal="true">
          <div className="cache-fullscreen-toolbar">
            {(filter === "moby" ||
              filter === "tie" ||
              filter === "detail") && (
              <span className="dim small mono">
                {selectedAsset?.name || selectedAsset?.asset_tuid}
              </span>
            )}
            {filter === "texture" && selectedTextureId && (
              <span className="dim small mono">id {selectedTextureId}</span>
            )}
            <button
              type="button"
              className="btn small"
              onClick={() => moveSelection(-1)}
              title="Previous (↑)"
              aria-label="Previous"
            >
              ↑
            </button>
            <button
              type="button"
              className="btn small"
              onClick={() => moveSelection(1)}
              title="Next (↓)"
              aria-label="Next"
            >
              ↓
            </button>
            <button
              type="button"
              className="btn small"
              onClick={() => setFullscreen(false)}
              title="Exit fullscreen (Esc)"
              aria-label="Exit fullscreen"
            >
              <Minimize2 size={14} />
            </button>
            <button
              type="button"
              className="cache-fullscreen-close"
              onClick={() => setFullscreen(false)}
              aria-label="Close"
              title="Close"
            >
              <X size={16} />
            </button>
          </div>
          <div className="cache-fullscreen-body">
            {(filter === "moby" ||
              filter === "tie" ||
              filter === "detail") &&
              selectedAsset && (
                <AssetPreview
                  instance={previewInstance}
                  meshes={previewMeshes}
                  textureBlobs={selectedTextures.size > 0 ? selectedTextures : null}
                  cacheFolder={folder ?? undefined}
                  exportPicks={previewPicks}
                  onExportPicksChange={setPreviewPicks}
                />
              )}
            {filter === "texture" && textureBlobUrl && (
              <img
                src={textureBlobUrl}
                alt={`texture ${selectedTextureId}`}
                className="cache-fullscreen-img"
              />
            )}
          </div>
        </div>
      )}
    </Modal>
  );
}

function SkyTexturePanel({
  folder,
  currentSkyboxTextureId,
  onPickTexture,
  onClearSkybox,
  onExportPng,
  onExportDds,
}: {
  folder: string | null;
  currentSkyboxTextureId: number | null;
  onPickTexture: () => void;
  onClearSkybox: () => void;
  onExportPng: () => void | Promise<void>;
  onExportDds: () => void | Promise<void>;
}) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!folder || currentSkyboxTextureId == null) {
      setUrl(null);
      return;
    }
    let cancelled = false;
    let created: string | null = null;
    readCachedBytes(folder, `textures/${currentSkyboxTextureId}.png`)
      .then((bytes) => {
        if (cancelled) return;
        const blob = new Blob([bytes as ArrayBuffer], { type: "image/png" });
        created = URL.createObjectURL(blob);
        setUrl(created);
      })
      .catch(() => {
        if (!cancelled) setUrl(null);
      });
    return () => {
      cancelled = true;
      if (created) URL.revokeObjectURL(created);
    };
  }, [folder, currentSkyboxTextureId]);

  return (
    <div
      className="cache-sky-panel"
      style={{
        padding: 16,
        display: "flex",
        flexDirection: "column",
        gap: 12,
        height: "100%",
        minHeight: 0,
      }}
    >
      <div>
        <h3 style={{ margin: "0 0 4px" }}>Cubemap</h3>
        <p className="dim small" style={{ margin: 0 }}>
          The level's auto-extracted cubemap (6 DXT1 faces from
          cubemaps.dat) binds as the scene background. Optionally pick a
          single cached texture as an equirect override here — applies on
          top and you can save it as PNG or DDS.
        </p>
      </div>
      <div
        style={{
          flex: 1,
          minHeight: 0,
          border: "1px solid var(--border-subtle, #2a2d33)",
          borderRadius: 6,
          background: "#0a0c10",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          overflow: "hidden",
        }}
      >
        {currentSkyboxTextureId == null ? (
          <span className="dim">No cubemap override set</span>
        ) : url ? (
          <img
            src={url}
            alt={`cubemap ${currentSkyboxTextureId}`}
            draggable={false}
            onDragStart={(e) => e.preventDefault()}
            style={{
              maxWidth: "100%",
              maxHeight: "100%",
              objectFit: "contain",
              imageRendering: "auto",
            }}
          />
        ) : (
          <span className="dim small">Loading…</span>
        )}
      </div>
      <div
        style={{
          display: "flex",
          gap: 8,
          flexWrap: "wrap",
          alignItems: "center",
        }}
      >
        <Button onClick={onPickTexture}>
          {currentSkyboxTextureId == null ? "Pick texture…" : "Change texture…"}
        </Button>
        {currentSkyboxTextureId != null && (
          <>
            <Button onClick={onClearSkybox}>Clear cubemap</Button>
            <Button icon={Download} onClick={() => void onExportPng()}>
              Save PNG
            </Button>
            <Button icon={Download} onClick={() => void onExportDds()}>
              Save DDS
            </Button>
            <span className="dim small mono" style={{ marginLeft: "auto" }}>
              id {currentSkyboxTextureId}
            </span>
          </>
        )}
      </div>
    </div>
  );
}

function Paginator({
  currentPage,
  totalPages,
  onChange,
}: {
  currentPage: number;
  totalPages: number;
  onChange: (next: number) => void;
}) {
  const [jumpInput, setJumpInput] = useState("");
  useEffect(() => {
    setJumpInput(String(currentPage + 1));
  }, [currentPage]);

  const maxDots = 12;
  const dots: number[] = [];
  if (totalPages <= maxDots) {
    for (let i = 0; i < totalPages; i++) dots.push(i);
  } else {
    const start = Math.max(0, Math.min(currentPage - 5, totalPages - maxDots));
    for (let i = 0; i < maxDots; i++) dots.push(start + i);
  }

  const commitJump = () => {
    const n = parseInt(jumpInput, 10);
    if (Number.isFinite(n)) {
      const target = Math.max(0, Math.min(totalPages - 1, n - 1));
      if (target !== currentPage) onChange(target);
      setJumpInput(String(target + 1));
    } else {
      setJumpInput(String(currentPage + 1));
    }
  };

  return (
    <div className="cache-paginator">
      <button
        type="button"
        className="cache-paginator-arrow"
        onClick={() => onChange(Math.max(0, currentPage - 1))}
        disabled={currentPage === 0}
        aria-label="Previous page"
      >
        ‹
      </button>
      <div className="cache-paginator-dots">
        {dots.map((p) => (
          <button
            key={p}
            type="button"
            className={`cache-paginator-dot${p === currentPage ? " is-current" : ""}`}
            onClick={() => onChange(p)}
            aria-label={`Page ${p + 1}`}
            title={`Page ${p + 1}`}
          />
        ))}
      </div>
      <div className="cache-paginator-jump">
        <input
          type="text"
          inputMode="numeric"
          className="cache-paginator-jump-input mono"
          value={jumpInput}
          onChange={(e) => setJumpInput(e.target.value.replace(/[^0-9]/g, ""))}
          onBlur={commitJump}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.currentTarget.blur();
            } else if (e.key === "Escape") {
              setJumpInput(String(currentPage + 1));
              e.currentTarget.blur();
            }
          }}
          aria-label="Jump to page"
        />
        <span className="dim small mono">/ {totalPages}</span>
      </div>
      <button
        type="button"
        className="cache-paginator-arrow"
        onClick={() => onChange(Math.min(totalPages - 1, currentPage + 1))}
        disabled={currentPage >= totalPages - 1}
        aria-label="Next page"
      >
        ›
      </button>
    </div>
  );
}

function TextureTile({
  folder,
  entry,
  active,
  multi,
  onClick,
}: {
  folder: string | null;
  entry: CacheManifestEntry;
  active: boolean;
  multi: boolean;
  onClick: (mods: { ctrl: boolean; shift: boolean }) => void;
}) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!folder) return;
    let cancelled = false;
    let createdUrl: string | null = null;
    readCachedBytes(folder, entry.file)
      .then((bytes) => {
        if (cancelled) return;
        const blob = new Blob([bytes as ArrayBuffer], { type: "image/png" });
        createdUrl = URL.createObjectURL(blob);
        setUrl(createdUrl);
      })
      .catch(() => {
        if (cancelled) return;
        setUrl(null);
      });
    return () => {
      cancelled = true;
      if (createdUrl) URL.revokeObjectURL(createdUrl);
    };
  }, [folder, entry.file]);
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest" });
  }, [active]);
  return (
    <button
      ref={ref}
      type="button"
      className={`cache-texture-tile${active ? " is-active" : ""}${multi ? " is-multi" : ""}`}
      onClick={(e) =>
        onClick({ ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })
      }
      title={`${entry.tuid} · ${(entry.size_bytes / 1024).toFixed(0)} KB`}
    >
      <span className="cache-texture-tile-thumb">
        {url ? (
          <img
            src={url}
            alt={entry.tuid}
            loading="lazy"
            draggable={false}
            onDragStart={(e) => e.preventDefault()}
          />
        ) : (
          <span className="dim small">…</span>
        )}
      </span>
      <span className="cache-texture-tile-label mono small">
        {entry.tuid}
      </span>
    </button>
  );
}
