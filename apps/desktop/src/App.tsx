import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Panel,
  PanelGroup,
  PanelResizeHandle,
  type ImperativePanelHandle,
} from "react-resizable-panels";
import { open as openDialog } from "@tauri-apps/plugin-dialog";





import brandIconUrl from "../icon.png?url";
import { Channel } from "@tauri-apps/api/core";
import {
  cacheStatus,
  dumpSoundBank,
  loadCachedTextures,
  loadFromCache,
  readCachedManifest,
  reextractLevelCache,
  extractLevelSounds,
  extractLevelStreamSounds,
  extractLevelToCache,
  extractRawStreamingSounds,
  levelLayout,
  listAnimsetClips,
  listEntitiesGltfs,
  listGltfsInFolder,
  listLevelFiles,
  listLevelSounds,
  openLevel,
  readCachedAsset,
  wavBlobUrl,
  type AnimsetSummary,
  type CacheEvent,
  type CacheStatus,
  type CubemapDescriptor,
  type ExtractedSound,
  type GltfFile,
  type Instance,
  type LevelFile,
  type LevelMeshes,
  type LevelSummary,
  type AssetMeshes,
  type PhaseId,
  type SoundEntry,
  type TextureBlobMap,
  type UFragBounds,
} from "./api";
import { AboutModal } from "./components/AboutModal";
import { DocsModal } from "./components/DocsModal";
import { BottomPanel, type ConsoleEntry } from "./views/BottomPanel";
import { CacheLibraryModal } from "./components/CacheLibraryModal";
import { GltfCharacterModal } from "./components/GltfCharacterModal";
import { Hierarchy } from "./views/Hierarchy";
import { Inspector } from "./views/Inspector";
import { LoadProgress, type LoadPhaseState } from "./components/LoadProgress";
import { Menu, MenuBar, MenuCheckItem, MenuItem, MenuSpacer } from "./views/MenuBar";
import { Modal } from "./components/Modal";
import { OpenLevelModal } from "./components/OpenLevelModal";
import { PsarcModal } from "./components/PsarcModal";
import { AssetLookupModal } from "./components/AssetLookupModal";
import { SettingsModal } from "./components/SettingsModal";
import { TabContainer } from "./views/TabContainer";
import { useApplySettings } from "./useApplySettings";
import { useGlobalUiSounds } from "./useUiSound";
import type { ViewId } from "./store";
import { SoundPlayer, type NowPlaying } from "./components/SoundPlayer";
import { Splash } from "./views/Splash";
import { UpdateChecker } from "./components/UpdateChecker";
import { useUpdater } from "./useUpdater";
import { WhatsNewModal } from "./components/WhatsNewModal";
import { useWhatsNew } from "./useWhatsNew";
import { StatusBar } from "./views/StatusBar";
import { TitleBar } from "./views/TitleBar";
import { Toolbar } from "./views/Toolbar";
import { Viewport } from "./views/Viewport";
import { AssetWorkbench } from "./views/AssetWorkbench";
import { useEdits } from "./edits";
import {
  exportToGlb,
  pickGlbExportPath,
  type ExportProgressState,
} from "./export";
import { ExportProgress } from "./components/ExportProgress";
import { useSelection } from "./selection";
import { APP_VERSION, APP_BRAND_NAME, APP_REPO_URL, APP_ISSUES_URL, openExternal } from "./version";
import {
  resetAll,
  setBottomPct,
  setHierarchyPct,
  setInspectorPct,
  toggleConsoleCollapsed,
  toggleHierarchyHidden,
  toggleInspectorHidden,
  toggleView,
  setSkybox,
  addTabToPanel,
  moveTab,
  setActiveTab,
  useAppDispatch,
  useAppSelector,
  type BooleanViewKey,
} from "./store";

export function App() {
  const { t } = useTranslation();
  const dispatch = useAppDispatch();
  const view = useAppSelector((s) => s.view);
  const layout = useAppSelector((s) => s.layout);

  useApplySettings();
  useGlobalUiSounds();

  const leftPanelTabs = useAppSelector(
    (s) => s.panels.panels.left.tabs.length,
  );
  const rightPanelTabs = useAppSelector(
    (s) => s.panels.panels.right.tabs.length,
  );
  const bottomPanelTabs = useAppSelector(
    (s) => s.panels.panels.bottom.tabs.length,
  );

  useEffect(() => {
    const empty = leftPanelTabs === 0;
    if (empty && !layout.hierarchyHidden) dispatch(toggleHierarchyHidden());
    if (!empty && layout.hierarchyHidden) dispatch(toggleHierarchyHidden());
  }, [leftPanelTabs]);

  useEffect(() => {
    const empty = rightPanelTabs === 0;
    if (empty && !layout.inspectorHidden) dispatch(toggleInspectorHidden());
    if (!empty && layout.inspectorHidden) dispatch(toggleInspectorHidden());
  }, [rightPanelTabs]);

  useEffect(() => {
    const empty = bottomPanelTabs === 0;
    if (empty && !layout.consoleCollapsed) dispatch(toggleConsoleCollapsed());
    if (!empty && layout.consoleCollapsed) dispatch(toggleConsoleCollapsed());
  }, [bottomPanelTabs]);

  useEffect(() => {
    const panel = bottomPanelRef.current;
    if (!panel) return;
    if (layout.consoleCollapsed) {
      if (!panel.isCollapsed()) panel.collapse();
    } else {
      if (panel.isCollapsed()) panel.expand();
    }
  }, [layout.consoleCollapsed]);

  useEffect(() => {
    const panel = hierarchyPanelRef.current;
    if (!panel) return;
    if (layout.hierarchyHidden) {
      if (!panel.isCollapsed()) panel.collapse();
    } else {
      if (panel.isCollapsed()) panel.expand();
    }
  }, [layout.hierarchyHidden]);

  useEffect(() => {
    const panel = inspectorPanelRef.current;
    if (!panel) return;
    if (layout.inspectorHidden) {
      if (!panel.isCollapsed()) panel.collapse();
    } else {
      if (panel.isCollapsed()) panel.expand();
    }
  }, [layout.inspectorHidden]);

  const [summary, setSummary] = useState<LevelSummary | null>(null);
  const [openedGame, setOpenedGame] = useState<string | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [ufrags, setUFrags] = useState<UFragBounds[]>([]);
  const [meshes, setMeshes] = useState<LevelMeshes | null>(null);
  
  
  
  
  
  const [textureBlobs, setTextureBlobs] = useState<TextureBlobMap | null>(null);
  const [cubemapDescriptor, setCubemapDescriptor] =
    useState<CubemapDescriptor | null>(null);
  const selection = useSelection(useCallback(() => instances, [instances]));
  const edits = useEdits();
  const bottomPanelRef = useRef<ImperativePanelHandle>(null);
  const hierarchyPanelRef = useRef<ImperativePanelHandle>(null);
  const inspectorPanelRef = useRef<ImperativePanelHandle>(null);
  const primaryInstance = selection.primary
    ? instances.find((i) => i.tuid === selection.primary) ?? null
    : null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.tagName === "SELECT" ||
          t.isContentEditable)
      ) {
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
      if (!selection.primary) return;
      switch (e.key.toLowerCase()) {
        case "g":
          edits.setMode("translate");
          e.preventDefault();
          break;
        case "r":
          edits.setMode("rotate");
          e.preventDefault();
          break;
        case "s":
          edits.setMode("scale");
          e.preventDefault();
          break;
        case "f":
          setFocusVersion((v) => v + 1);
          e.preventDefault();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selection.primary, edits.setMode]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadPhase, setLoadPhase] = useState<LoadPhaseState | null>(null);
  const [meshLoadPhase, setMeshLoadPhase] = useState<LoadPhaseState | null>(null);
  const [completedPhases, setCompletedPhases] = useState<PhaseId[]>([]);
  const [consoleLog, setConsoleLog] = useState<ConsoleEntry[]>([]);
  const updater = useUpdater();
  const whatsNew = useWhatsNew();



  const [aboutModalOpen, setAboutModalOpen] = useState(false);
  const [docsModalOpen, setDocsModalOpen] = useState(false);
  const [openLevelModalOpen, setOpenLevelModalOpen] = useState(false);
  const [psarcModalOpen, setPsarcModalOpen] = useState(false);
  const [assetLookupModalOpen, setAssetLookupModalOpen] = useState(false);
  const [settingsModalOpen, setSettingsModalOpen] = useState(false);
  const [exportState, setExportState] = useState<ExportProgressState | null>(null);
  
  
  
  
  
  const [_gltfLibrary, setGltfLibrary] = useState<GltfFile[] | null>(null);
  const [_gltfLibraryStatus, setGltfLibraryStatus] = useState<string | null>(null);
  const [previewGltfFile, setPreviewGltfFile] = useState<GltfFile | null>(null);
  
  
  
  
  const [_animsetClips, setAnimsetClips] = useState<AnimsetSummary[]>([]);
  
  
  
  
  const [overrideAnimsetHash, setOverrideAnimsetHash] = useState<string | null>(null);
  
  
  
  
  
  const [previewAssetTuid, setPreviewAssetTuid] = useState<string | null>(null);
  
  
  
  
  const [levelSounds, setLevelSounds] = useState<SoundEntry[]>([]);
  
  
  
  
  
  const [_levelFiles, setLevelFiles] = useState<LevelFile[]>([]);
  
  
  
  
  
  const [cacheState, setCacheState] = useState<CacheStatus | null>(null);
  
  
  
  
  const [cacheManifest, setCacheManifest] =
    useState<import("./api").CacheManifest | null>(null);
  const [cacheProgress, setCacheProgress] = useState<{
    phase: "mobys" | "ties" | "materials" | "normalmaps" | "textures" | "ufrags";
    current: number;
    total: number;
  } | null>(null);
  const [cacheLibraryOpen, setCacheLibraryOpen] = useState(false);
  const [cacheModalInitialPanel, setCacheModalInitialPanel] =
    useState<import("./components/CacheLibraryModal").LibraryFilter | null>(null);
  const [cacheModalInitialTextureId, setCacheModalInitialTextureId] =
    useState<string | null>(null);
  const [cacheModalInitialSoundKey, setCacheModalInitialSoundKey] =
    useState<string | null>(null);
  // Override input for the AssetWorkbench tab. When set (by the modal's
  // "open in workbench" button or the Inspector's view-model button), it
  // takes precedence over the selection-derived primaryInstance so the
  // workbench can show an asset the user picked from the cache library
  // even if no instance of it is selected in the level.
  const [workbenchAsset, setWorkbenchAsset] = useState<
    { tuid: string; kind: "moby" | "tie" } | null
  >(null);
  const panelsState = useAppSelector((s) => s.panels.panels);
  const openInWorkbench = useCallback(
    (tuid: string, kind: "moby" | "tie") => {
      setWorkbenchAsset({ tuid, kind });
      // Surface the tab in the center panel. If it lives in another
      // panel, move it; otherwise create it. Then activate.
      const ownerPanel = (
        ["left", "right", "bottom", "center"] as const
      ).find((p) => panelsState[p]?.tabs.includes("assetWorkbench"));
      if (ownerPanel === "center") {
        dispatch(setActiveTab({ panelId: "center", viewId: "assetWorkbench" }));
      } else if (ownerPanel) {
        dispatch(
          moveTab({
            viewId: "assetWorkbench",
            from: ownerPanel,
            to: "center",
          }),
        );
      } else {
        dispatch(
          addTabToPanel({ panelId: "center", viewId: "assetWorkbench" }),
        );
      }
    },
    [dispatch, panelsState],
  );
  
  
  
  
  const [cachePrompt, setCachePrompt] = useState<{
    sum: LevelSummary;
    status: CacheStatus;
  } | null>(null);
  
  
  
  
  const [extractedSoundsCache, setExtractedSoundsCache] = useState<
    Map<string, ExtractedSound[]>
  >(new Map());
  
  
  
  
  
  const [nowPlaying, setNowPlaying] = useState<NowPlaying | null>(null);
  void nowPlaying;

  
  

  
  
  
  
  
  const nowPlayingMirror = useRef<NowPlaying | null>(null);
  nowPlayingMirror.current = nowPlaying;
  useEffect(() => {
    return () => {
      const np = nowPlayingMirror.current;
      if (np) {
        np.audio.pause();
        URL.revokeObjectURL(np.blobUrl);
      }
    };
  }, []);
  
  
  
  
  const [focusVersion, setFocusVersion] = useState(0);
  const [viewSnap] = useState<{
    direction: "front" | "right" | "top" | null;
    version: number;
  }>({ direction: null, version: 0 });
  
  
  
  const [splashVisible, setSplashVisible] = useState(true);
  const [splashMounted, setSplashMounted] = useState(true);
  useEffect(() => {
    const id = setTimeout(() => setSplashVisible(false), 1200);
    return () => clearTimeout(id);
  }, []);

  const autoOpenLevelRef = useRef(false);
  useEffect(() => {
    if (autoOpenLevelRef.current) return;
    if (splashVisible) return;
    if (summary) return;
    autoOpenLevelRef.current = true;
    setOpenLevelModalOpen(true);
  }, [splashVisible, summary]);

  const log = useCallback(
    (level: ConsoleEntry["level"], msg: string) =>
      setConsoleLog((cur) => [...cur, { ts: Date.now(), level, msg }]),
    [],
  );

  useEffect(() => {
    if (error) log("error", error);
  }, [error, log]);

  const handleExportSelection = useCallback(async () => {
    console.log("[export:inspector] click", {
      selectionSize: selection.ids.size,
      hasMeshes: !!meshes,
      mobyAssets: meshes?.moby_assets.length ?? 0,
      tieAssets: meshes?.tie_assets.length ?? 0,
      textureBlobs: textureBlobs?.size ?? 0,
      overrideAnimsetHash,
    });
    if (selection.ids.size === 0 || !meshes) {
      console.warn("[export:inspector] aborted — no selection or meshes");
      return;
    }

    let path: string | null = null;
    try {
      const selectedInstances = instances.filter((i) =>
        selection.ids.has(i.tuid),
      );
      path = await pickGlbExportPath(selectedInstances);
    } catch (err) {
      console.error("[export:inspector] save dialog failed", err);
      log("error", `Save dialog failed: ${err}`);
      return;
    }
    if (!path) {
      console.log("[export:inspector] cancelled — no path picked");
      log("info", "Export cancelled");
      return;
    }

    console.log("[export:inspector] using FE pipeline (exportToGlb)", { path });
    setExportState({
      phase: "preparing",
      label: "Building scene from selection",
      fraction: 0,
      detail: path,
    });
    try {
      const result = await exportToGlb(
        selection.ids,
        instances,
        meshes,
        textureBlobs,
        path,
        (state) => setExportState(state),
        summary?.folder ?? null,
        overrideAnimsetHash,
      );
      console.log("[export:inspector] success", {
        bytes: result.bytes,
        path: result.path,
      });
      log(
        "ok",
        `Exported ${result.bytes.toLocaleString()} bytes → ${result.path}`,
      );
    } catch (err) {
      console.error("[export:inspector] failed", err);
      log("error", `Export failed: ${err}`);
      setExportState({
        phase: "done",
        label: `Export failed: ${err}`,
        fraction: 0,
        cancelled: true,
      });
    }
  }, [selection.ids, instances, meshes, log]);

  
  
  
  
  
  
  
  const toggle = useCallback(
    (key: BooleanViewKey) => dispatch(toggleView(key)),
    [dispatch],
  );

  
  
  
  
  
  
  const handlePlaySound = useCallback(
    async (name: string) => {
      
      if (nowPlayingMirror.current?.name === name) {
        const np = nowPlayingMirror.current;
        np.audio.pause();
        URL.revokeObjectURL(np.blobUrl);
        setNowPlaying(null);
        return;
      }
      
      if (nowPlayingMirror.current) {
        const prev = nowPlayingMirror.current;
        prev.audio.pause();
        URL.revokeObjectURL(prev.blobUrl);
      }

      if (!summary) return;
      
      
      
      const entry = levelSounds.find((s) => s.name === name);
      if (!entry) {
        log("warn", `Sound metadata missing: ${name}`);
        return;
      }
      const cacheKey = `${entry.kind}:${entry.source}`;
      let extracted: ExtractedSound[];
      const cached = extractedSoundsCache.get(cacheKey);
      if (cached) {
        extracted = cached;
      } else {
        try {
          
          
          
          
          
          
          if (entry.kind === "raw") {
            extracted = await extractRawStreamingSounds(summary.folder, entry.source);
            log(
              "ok",
              `Raw streams decoded from ${entry.source}: ${extracted.length} WAVs`,
            );
          } else if (entry.kind === "stream") {
            extracted = await extractLevelStreamSounds(summary.folder, entry.source);
            log(
              "ok",
              `Stream sounds decoded for ${entry.source}: ${extracted.length} WAVs`,
            );
          } else {
            extracted = await extractLevelSounds(summary.folder);
            log("ok", `Sound bank decoded: ${extracted.length} WAVs`);
          }
          const fresh = extracted;
          setExtractedSoundsCache((prev) => {
            const next = new Map(prev);
            next.set(cacheKey, fresh);
            return next;
          });
        } catch (e) {
          log("error", `Sound extract failed: ${e}`);
          
          
          
          
          if (entry.kind !== "raw" && summary) {
            dumpSoundBank(summary.folder, entry.source)
              .then((dump) => log("info", `Bank dump for ${entry.source}:\n${dump}`))
              .catch((de) => log("warn", `Bank dump failed: ${de}`));
          }
          return;
        }
      }

      const found = extracted.find((s) => s.name === name);
      if (!found) {
        log("warn", `Sound not found: ${name}`);
        return;
      }
      const blobUrl = wavBlobUrl(found.wav_b64);
      const audio = new Audio(blobUrl);
      audio.addEventListener("ended", () => {
        
        
        
      });
      audio.play().catch((e) => log("error", `Audio play failed: ${e}`));
      setNowPlaying({
        name,
        source: entry.source,
        audio,
        blobUrl,
        entry: found,
      });
    },
    [extractedSoundsCache, levelSounds, summary, log],
  );
  void handlePlaySound;

  



  const handleBrowseGltfFolder = useCallback(async () => {
    let picked: string | null = null;
    try {
      const result = await openDialog({
        directory: true,
        multiple: false,
        title: "Pick a GLTF library folder",
      });
      picked = typeof result === "string" ? result : null;
    } catch (err) {
      log("error", `GLTF folder picker failed: ${err}`);
      return;
    }
    if (!picked) return;

    setGltfLibraryStatus(`Scanning ${picked}…`);
    try {
      const lib = await listGltfsInFolder(picked);
      if (lib.files.length === 0) {
        log("warn", `No .gltf/.glb files in ${picked}`);
        setGltfLibrary([]);
      } else {
        log("ok", `Loaded ${lib.files.length} GLTF files from ${picked}`);
        setGltfLibrary(lib.files);
      }
    } catch (e) {
      log("error", `GLTF scan failed: ${e}`);
      setGltfLibrary(null);
    } finally {
      setGltfLibraryStatus(null);
    }
  }, [log]);

  
  
  
  
  
  
  type CacheMode = "auto" | "use-cache" | "force-reextract";

  const runCacheExtract = useCallback(
    (folder: string, force: boolean): Promise<void> => {
      return new Promise((resolve, reject) => {
        log(
          "info",
          force
            ? "Rebuilding disk cache (user-requested re-extract)…"
            : "Extracting level to disk cache…",
        );
        const channel = new Channel<CacheEvent>();
        let phaseTotals = { phase: "mobys" as const, total: 0 };
        channel.onmessage = (event) => {
          switch (event.type) {
            case "phase":
              phaseTotals = { phase: event.phase, total: event.total } as never;
              setCacheProgress({
                phase: event.phase,
                current: 0,
                total: event.total,
              });
              break;
            case "item":
              // ignored — we read everything via loadFromCache after `done`
              break;
            case "progress":
              setCacheProgress({
                phase: phaseTotals.phase,
                current: event.current,
                total: phaseTotals.total,
              });
              break;
            case "done":
              setCacheProgress(null);
              cacheStatus(folder).then(setCacheState).catch(() => {});
              log("ok", `Cache built: ${event.entry_count} entries`);
              resolve();
              break;
            case "error":
              setCacheProgress(null);
              log("warn", `Cache extraction failed: ${event.message}`);
              reject(new Error(event.message));
              break;
          }
        };
        const fn = force ? reextractLevelCache : extractLevelToCache;
        fn(folder, channel).catch((e) => {
          setCacheProgress(null);
          log("warn", `Cache extraction failed: ${e}`);
          reject(e instanceof Error ? e : new Error(String(e)));
        });
      });
    },
    [log],
  );

  const loadFullMeshes = useCallback(
    async (sum: LevelSummary, cacheMode: CacheMode = "auto") => {
      setError(null);
      setBusy(true);
      setMeshes(null);
      setTextureBlobs(null);
      setCubemapDescriptor(null);
      setMeshLoadPhase({
        phase: "layout",
        label: "Preparing extraction",
        current: 0,
        total: 0,
        chunkSize: 1,
      });
      setCompletedPhases([]);

      try {
        let needExtract = cacheMode === "force-reextract";
        if (!needExtract) {
          const status = await cacheStatus(sum.folder);
          setCacheState(status);
          needExtract = !status.exists || status.incomplete;
        }

        if (needExtract) {
          await runCacheExtract(sum.folder, cacheMode === "force-reextract");
          const status = await cacheStatus(sum.folder);
          setCacheState(status);
        }
        const manifest = await readCachedManifest(sum.folder);
        setCacheManifest(manifest);
        setMeshLoadPhase({
          phase: "mobys",
          label: "Loading from cache",
          current: 0,
          total: 1,
          chunkSize: 1,
        });
        const phaseLabel: Record<string, string> = {
          manifest: "Reading manifest",
          mobys: "Loading mobys",
          ties: "Loading ties",
          ufrags: "Loading terrain",
          textures: "Loading textures",
        };
        const meshes = await loadFromCache(sum.folder, (p) => {
          setMeshLoadPhase({
            phase: p.phase === "manifest" ? "mobys" : (p.phase as PhaseId),
            label: phaseLabel[p.phase] ?? p.phase,
            current: p.current,
            total: Math.max(p.total, 1),
            chunkSize: 1,
          });
        });
        setMeshes(meshes);
        const cubeEntry = manifest.entries.find((e) => e.kind === "cubemap");
        if (cubeEntry) {
          readCachedAsset(sum.folder, cubeEntry.file)
            .then((d) => setCubemapDescriptor(d as CubemapDescriptor))
            .catch(() => setCubemapDescriptor(null));
        }

        log(
          "ok",
          `Loaded from cache: ${meshes.moby_assets.length} mobys, ${meshes.tie_assets.length} ties, ${meshes.ufrag_meshes.length} terrain, ${meshes.textures.length} textures, ${meshes.moby_assets.reduce((s, a) => s + (a.embedded_animation_count ?? 0), 0)} anims`,
        );

        // Old flow: meshes render first (dim / untextured), THEN
        // the full texture set lands as one batch. No progressive
        // onBatch updates — `useTextureMap` in the viewport binds
        // them all in one pass when the Map identity changes.
        const ids = meshes.textures.map((t) => t.id);
        if (ids.length > 0) {
          loadCachedTextures(sum.folder, ids, { concurrency: 32 })
            .then((blobs) => setTextureBlobs(blobs))
            .catch((err) =>
              log("error", `Texture cache fetch failed: ${err}`),
            );
        } else {
          setTextureBlobs(new Map());
        }

        setMeshLoadPhase(null);
      } catch (e) {
        console.error("[loadFullMeshes] failed", e);
        setError(`Cache load failed: ${e}`);
        setMeshLoadPhase(null);
      } finally {
        setBusy(false);
      }
    },
    [log, runCacheExtract],
  );


  const handleOpen = useCallback(async (rawFolder: string, opts?: { skipCachePrompt?: boolean; game?: string }) => {
    const folder = rawFolder.trim();
    if (!folder) return;
    setOpenedGame(opts?.game ?? null);
    setOpenLevelModalOpen(false);
    setError(null);
    setBusy(true);
    selection.clear();
    edits.resetAll();
    setMeshes(null);
    setTextureBlobs(null);
    setGltfLibrary(null);
    setGltfLibraryStatus(null);
    setPreviewGltfFile(null);
    setAnimsetClips([]);
    setOverrideAnimsetHash(null);
    setPreviewAssetTuid(null);


    if (nowPlayingMirror.current) {
      nowPlayingMirror.current.audio.pause();
      URL.revokeObjectURL(nowPlayingMirror.current.blobUrl);
    }
    setNowPlaying(null);
    setLevelSounds([]);
    setExtractedSoundsCache(new Map());
    setLevelFiles([]);
    setLoadPhase(null);
    setCompletedPhases([]);
    setMeshLoadPhase({
      phase: "layout",
      label: "Reading level header",
      current: 0,
      total: 0,
      chunkSize: 1,
    });
    log("info", `Opening level: ${folder}`);
    try {
      const sum = await openLevel(folder);
      setSummary(sum);
      log(
        "ok",
        `IGHW v${sum.version_major}.${sum.version_minor} · ${sum.sections.length} sections`,
      );
      const lyt = await levelLayout(sum.folder);
      
      
      
      const seen = new Set<string>();
      const dedupedInstances = lyt.instances.filter((i) => {
        if (seen.has(i.tuid)) return false;
        seen.add(i.tuid);
        return true;
      });
      const dropped = lyt.instances.length - dedupedInstances.length;
      setInstances(dedupedInstances);
      setUFrags(lyt.ufrags);
      log(
        "info",
        `Layout: ${dedupedInstances.length} instances` +
          (dropped > 0 ? ` (deduped ${dropped})` : "") +
          `, ${lyt.ufrags.length} UFrags`,
      );

      setBusy(false);

      // Kick off the full mesh decode automatically. Safe now that:
      //   1. Texture uploads skip mipmap generation (was the #1 cause of
      //      WebGL context loss on bayou)
      //   2. Geometry build runs in `requestIdleCallback` slots, one
      //      asset at a time, off the render path
      //   3. `computeVertexNormals` is skipped (~150ms saved per moby)
      //   4. WebGL context-loss handler shows a banner instead of
      //      letting the canvas silently freeze
      //
      // The decode runs concurrently with idle-time mesh building, so
      // the proxy-box view stays responsive while real meshes stream in
      // and progressively replace the boxes. No await — let it run in
      // the background.
      // Probe for an existing cache BEFORE the streaming pipeline starts.
      // If the level was already extracted, ask the user whether to use the
      // cached data or rebuild from scratch — cheaper than always extracting,
      // and lets them recover from a stale/corrupt cache without leaving the
      // app. If no cache exists yet, the auto path runs the legacy flow.
      let existing: CacheStatus | null = null;
      try {
        existing = await cacheStatus(sum.folder);
      } catch (e) {
        log("warn", `Cache status check failed: ${e}`);
      }
      if (existing && existing.exists) {
        if (opts?.skipCachePrompt) {
          log(
            "info",
            `Cache detected (${existing.mobys}M / ${existing.ties}T / ${existing.textures}tex) — auto-using (skipCachePrompt)`,
          );
          void loadFullMeshes(sum, "use-cache");
        } else {
          log(
            "info",
            `Cache detected (${existing.mobys}M / ${existing.ties}T / ${existing.textures}tex) — awaiting user choice`,
          );
          setMeshLoadPhase(null);
          setCachePrompt({ sum, status: existing });
        }
      } else {
        log("info", "Auto-loading meshes (idle-paced; safe to interact while it runs)");
        void loadFullMeshes(sum, "auto");
      }

      
      
      
      
      
      listAnimsetClips(sum.folder)
        .then((clips) => {
          setAnimsetClips(clips);
          log("ok", `Animset library: ${clips.length} clips`);
        })
        .catch((e) => {
          log("warn", `Animset list failed: ${e}`);
        });

      
      
      
      listLevelSounds(sum.folder)
        .then((sounds) => {
          setLevelSounds(sounds);
          if (sounds.length > 0) {
            log("ok", `Sound bank: ${sounds.length} entries`);
          }
        })
        .catch((e) => {
          log("warn", `Sound list failed: ${e}`);
        });

      
      
      
      
      listLevelFiles(sum.folder)
        .then((files) => {
          setLevelFiles(files);
          if (files.length > 0) {
            const parsedCount = files.filter((f) => f.parsed).length;
            log(
              "info",
              `Level files: ${files.length} total · ${parsedCount} parsed · ${files.length - parsedCount} roadmap`,
            );
          }
        })
        .catch((e) => {
          log("warn", `File listing failed: ${e}`);
        });

      
      
      
      
      
      
      
      
      
      
      

      
      
      
      
      
      
      
      void setGltfLibrary;
      void setGltfLibraryStatus;
      void listEntitiesGltfs;
    } catch (e) {
      setError(String(e));
      setSummary(null);
      setInstances([]);
      setUFrags([]);
      setLoadPhase(null);
      setMeshLoadPhase(null);
    } finally {
      setBusy(false);
    }
  }, [log, selection, edits, loadFullMeshes]);

  
  
  const resolveCachePrompt = useCallback(
    (mode: "use-cache" | "force-reextract") => {
      const pending = cachePrompt;
      if (!pending) return;
      setCachePrompt(null);
      log(
        "info",
        mode === "use-cache"
          ? "Loading meshes — will reuse the existing cache"
          : "Loading meshes — will rebuild the cache after streaming",
      );
      void loadFullMeshes(pending.sum, mode);
    },
    [cachePrompt, loadFullMeshes, log],
  );

  const handleClose = useCallback(() => {
    setSummary(null);
    setOpenedGame(null);
    setInstances([]);
    setUFrags([]);
    setMeshes(null);
    setTextureBlobs(null);
    setGltfLibrary(null);
    setGltfLibraryStatus(null);
    setPreviewGltfFile(null);
    setPreviewAssetTuid(null);
    setAnimsetClips([]);
    setOverrideAnimsetHash(null);
    setCacheState(null);
    setCacheManifest(null);
    setCacheProgress(null);
    setCachePrompt(null);
    selection.clear();
    edits.resetAll();
    setError(null);
    setLoadPhase(null);
    setMeshLoadPhase(null);
    setCompletedPhases([]);
    log("info", "Level closed");
  }, [log, selection, edits]);

  // Friendly map name derived from the level folder. V2 levels nest as
  // `…/packed/levels/<map>/built/levels/<map>`; RFOM/TOD point at the level
  // dir directly. Walk up skipping the generic `built`/`levels` segments and
  // take the first meaningful name.
  const mapName = useMemo(() => {
    if (!summary?.folder) return null;
    const parts = summary.folder
      .replace(/\\/g, "/")
      .split("/")
      .filter((p) => p && p !== "built" && p !== "levels");
    return parts[parts.length - 1] ?? null;
  }, [summary?.folder]);

  // Distinct (albedo, normal, emissive) shader triples across every submesh —
  // matches what the viewport material cache keys by, so the count reflects
  // what actually renders as a separate material instance.
  const materialCount = useMemo(() => {
    if (!meshes) return 0;
    const seen = new Set<string>();
    const visit = (assets: AssetMeshes[]) => {
      for (const a of assets) {
        for (const s of a.submeshes) {
          seen.add(`${s.albedo_id ?? "_"}|${s.normal_id ?? "_"}|${s.emissive_id ?? "_"}`);
        }
      }
    };
    visit(meshes.moby_assets);
    visit(meshes.tie_assets);
    if (meshes.detail_assets) visit(meshes.detail_assets);
    if (meshes.shrub_assets) visit(meshes.shrub_assets);
    if (meshes.foliage_assets) visit(meshes.foliage_assets);
    for (const u of meshes.ufrag_meshes) {
      seen.add(`${u.mesh.albedo_id ?? "_"}|${u.mesh.normal_id ?? "_"}|${u.mesh.emissive_id ?? "_"}`);
    }
    return seen.size;
  }, [meshes]);

  // Total animation clips embedded across all mobys (ties/ufrags have none).
  const animationCount = useMemo(() => {
    if (!meshes) return 0;
    return meshes.moby_assets.reduce(
      (sum, a) => sum + (a.embedded_animation_count ?? 0),
      0,
    );
  }, [meshes]);

  const toolbarInfo = meshes
    ? `${meshes.moby_assets.length} mobys · ${meshes.tie_assets.length} ties · ${meshes.ufrag_meshes.length} terrain · ${materialCount} materials · ${meshes.textures.length} textures · ${animationCount} anims`
    : summary
      ? `${instances.length.toLocaleString()} instances · ${ufrags.length.toLocaleString()} UFrags`
      : "";

  const errorCount = consoleLog.filter((e) => e.level === "error").length;
  const warnCount = consoleLog.filter((e) => e.level === "warn").length;

  const viewBodies: Partial<Record<ViewId, React.ReactNode>> = {
    hierarchy: (
      <Hierarchy
        instances={instances}
        selection={selection}
        cacheManifest={cacheManifest}
        sounds={levelSounds}
        onPreviewRawAsset={(tuid) => setPreviewAssetTuid(tuid)}
        onSelectCacheSound={(key) => {
          setCacheModalInitialPanel("sound");
          setCacheModalInitialSoundKey(key);
          setCacheLibraryOpen(true);
        }}
        onSelectCacheTexture={(texId) => {
          setCacheModalInitialPanel("texture");
          setCacheModalInitialTextureId(texId);
          setCacheLibraryOpen(true);
        }}
      />
    ),
    inspector: (
      <Inspector
        selected={primaryInstance}
        selectionCount={selection.count}
        meshes={meshes}
        textureBlobs={textureBlobs}
        instances={instances}
        edits={edits}
        cacheFolder={summary?.folder ?? null}
        onExportSelected={handleExportSelection}
        onLoadMeshes={() => {
          if (summary) void loadFullMeshes(summary);
        }}
        loadingMeshes={meshLoadPhase !== null}
        onFocusSelected={() => setFocusVersion((v) => v + 1)}
        onOpenInWorkbench={openInWorkbench}
      />
    ),
    console: (
      <BottomPanel
        summary={summary}
        console={consoleLog}
        collapsed={false}
        errorCount={errorCount}
        warnCount={warnCount}
      />
    ),
    viewport: (
      <div className="panel pane-viewport view-flush">
        <Viewport
          instances={instances}
          ufrags={ufrags}
          meshes={meshes}
          textureBlobs={textureBlobs}
          selection={selection}
          view={view}
          onToggle={toggle}
          focusVersion={focusVersion}
          viewSnap={viewSnap}
          edits={edits}
          meshLoadPhase={meshLoadPhase}
          levelFolder={summary?.folder ?? null}
          overrideAnimsetHash={overrideAnimsetHash}
          hasCachedSky={
            cacheManifest?.entries.some((e) => e.kind === "sky") ?? false
          }
          cacheVersion={cacheManifest?.entries.length ?? 0}
          cubemapDescriptor={cubemapDescriptor}
        />
      </div>
    ),
    assetWorkbench: (
      <div className="panel pane-viewport view-flush">
        <AssetWorkbench
          instance={primaryInstance}
          cacheFolder={summary?.folder ?? null}
          overrideAsset={workbenchAsset}
        />
      </div>
    ),
  };

  return (
    <div className="app">
      {splashMounted && (
        <Splash
          visible={splashVisible}
          onExit={() => setSplashMounted(false)}
        />
      )}
      <TitleBar>
        <MenuBar>
          <span
            className="brand"
            onClick={() => setAboutModalOpen(true)}
            role="button"
            tabIndex={0}
            title="About ReChimera"
          >
            <img
              src={brandIconUrl}
              alt=""
              className="brand-icon"
              draggable={false}
            />
            {APP_BRAND_NAME}
            <span className="brand-version mono small">v{APP_VERSION}</span>
          </span>

          {mapName && (
            <span className="open-level-badge" title={summary?.folder ?? undefined}>
              {openedGame && (
                <span className="open-level-badge-game mono">{openedGame}</span>
              )}
              <span className="open-level-badge-map mono">{mapName}</span>
            </span>
          )}

          <Menu label={t("menu.file")}>
            <MenuItem onSelect={() => setOpenLevelModalOpen(true)}>
              {t("menu.openLevel")}
            </MenuItem>
            <MenuItem onSelect={handleClose} disabled={!summary}>
              {t("menu.closeLevel")}
            </MenuItem>
          </Menu>

          <Menu label={t("menu.view")}>
            <MenuCheckItem
              checked={view.showGrid}
              onToggle={() => toggle("showGrid")}
            >
              {t("menu.grid")}
            </MenuCheckItem>
            <MenuCheckItem
              checked={view.showAxes}
              onToggle={() => toggle("showAxes")}
            >
              {t("menu.axes")}
            </MenuCheckItem>
            <MenuCheckItem
              checked={view.showStats}
              onToggle={() => toggle("showStats")}
            >
              {t("menu.fpsGraph")}
            </MenuCheckItem>
          </Menu>

          <Menu label="Render">
            <MenuItem
              onSelect={() => {
                if (summary) void loadFullMeshes(summary);
              }}
              disabled={!summary || busy || meshLoadPhase !== null}
            >
              {meshes ? "Reload Full Meshes" : "Load Full Meshes"}
            </MenuItem>
            <MenuCheckItem
              checked={view.showMobys}
              onToggle={() => toggle("showMobys")}
              disabled={!summary}
            >
              Mobys
            </MenuCheckItem>
            <MenuCheckItem
              checked={view.showTies}
              onToggle={() => toggle("showTies")}
              disabled={!summary}
            >
              Ties
            </MenuCheckItem>
            <MenuCheckItem
              checked={view.showDetails}
              onToggle={() => toggle("showDetails")}
              disabled={!summary}
            >
              Details
            </MenuCheckItem>
            <MenuCheckItem
              checked={view.showEnvSamplers}
              onToggle={() => toggle("showEnvSamplers")}
              disabled={!summary}
            >
              Env Probes
            </MenuCheckItem>
            <MenuCheckItem
              checked={view.showUFrags}
              onToggle={() => toggle("showUFrags")}
              disabled={!summary}
            >
              UFrag Terrain
            </MenuCheckItem>
            <MenuSpacer />
            <MenuItem
              onSelect={() => {
                setCacheModalInitialPanel("texture");
                setCacheLibraryOpen(true);
              }}
              disabled={!summary}
            >
              {view.skyboxTextureId != null
                ? `Cubemap: tex ${view.skyboxTextureId}`
                : "Cubemap: pick texture…"}
            </MenuItem>
            {view.skyboxTextureId != null && (
              <MenuItem onSelect={() => dispatch(setSkybox(null))}>
                Clear cubemap
              </MenuItem>
            )}
            <MenuCheckItem
              checked={view.showUFragBounds}
              onToggle={() => toggle("showUFragBounds")}
              disabled={!summary}
            >
              UFrag Bounds (debug)
            </MenuCheckItem>
          </Menu>

          <Menu label="Layout">
            <MenuItem onSelect={() => resetAll(dispatch)}>
              Reset to default
            </MenuItem>
          </Menu>

          <Menu label="Tools">
            <MenuItem onSelect={() => setPsarcModalOpen(true)}>
              {t("menu.extractPsarc")}
            </MenuItem>
            <MenuItem onSelect={() => setAssetLookupModalOpen(true)}>
              {t("menu.extractAssetLookup")}
            </MenuItem>
            <MenuItem onSelect={() => handleBrowseGltfFolder()}>
              Browse GLTF folder…
            </MenuItem>
            <MenuSpacer />
            <MenuItem onSelect={() => setSettingsModalOpen(true)}>
              {t("menu.settings")}
            </MenuItem>
          </Menu>

          <Menu label="Help">
            <MenuItem onSelect={() => setDocsModalOpen(true)}>
              Documentation…
            </MenuItem>
            <MenuItem onSelect={() => whatsNew.showManually()}>
              What's new…
            </MenuItem>
            <MenuItem onSelect={() => void openExternal(APP_REPO_URL)}>
              GitHub Repository
            </MenuItem>
            <MenuItem onSelect={() => void openExternal(APP_ISSUES_URL)}>
              Report an issue…
            </MenuItem>
            <MenuItem onSelect={() => setAboutModalOpen(true)}>
              About ReChimera…
            </MenuItem>
          </Menu>

          <MenuSpacer />

          <div className="menubar-panel-toggles" data-tauri-drag-region="false">
            <button
              className={`menubar-icon-btn ${layout.hierarchyHidden ? "" : "active"}`}
              onClick={() => dispatch(toggleHierarchyHidden())}
              title="Toggle left panel (Hierarchy)"
              aria-label="Toggle left panel"
              data-tauri-drag-region="false"
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
                <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                <rect x="1.5" y="2.5" width="4.5" height="11" rx="1.5" fill="currentColor" />
              </svg>
            </button>
            <button
              className={`menubar-icon-btn ${layout.consoleCollapsed ? "" : "active"}`}
              onClick={() => dispatch(toggleConsoleCollapsed())}
              title="Toggle bottom panel (Console)"
              aria-label="Toggle bottom panel"
              data-tauri-drag-region="false"
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
                <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                <rect x="1.5" y="9.5" width="13" height="4" rx="1.5" fill="currentColor" />
              </svg>
            </button>
            <button
              className={`menubar-icon-btn ${layout.inspectorHidden ? "" : "active"}`}
              onClick={() => dispatch(toggleInspectorHidden())}
              title="Toggle right panel (Inspector)"
              aria-label="Toggle right panel"
              data-tauri-drag-region="false"
            >
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden>
                <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                <rect x="10" y="2.5" width="4.5" height="11" rx="1.5" fill="currentColor" />
              </svg>
            </button>
          </div>

          <button
            className="menubar-icon-btn"
            onClick={() => setSettingsModalOpen(true)}
            title={t("menu.settings")}
            aria-label={t("menu.settings")}
            data-tauri-drag-region="false"
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden
            >
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </button>

          {updater.phase.kind === "available" && (
            <button
              className="btn btn-update"
              onClick={() => void updater.install()}
              title={
                updater.phase.manual
                  ? `v${updater.phase.update.version} available — opens GitHub Releases`
                  : `Update available — v${updater.phase.update.version}`
              }
              data-tauri-drag-region="false"
            >
              {updater.phase.manual ? "↗ Get latest on GitHub" : "↑ Update available"}
            </button>
          )}
          {updater.phase.kind === "downloading" && (
            <button
              className="btn"
              disabled
              data-tauri-drag-region="false"
            >
              Downloading update…
            </button>
          )}
        </MenuBar>
      </TitleBar>

      <Toolbar
        view={view}
        onToggle={toggle}
        hasLevel={summary != null}
        info={toolbarInfo}
        modifiedCount={edits.count}
        onResetAllEdits={edits.resetAll}
      />

      <div className="workspace">
        {





}
        <PanelGroup
            direction="horizontal"
            className="workspace-h"
          >
            <Panel
              id="panel-left"
              order={1}
              ref={hierarchyPanelRef}
              collapsible
              collapsedSize={0}
              defaultSize={layout.hierarchyHidden ? 0 : layout.hierarchyPct}
              minSize={10}
              maxSize={40}
              onResize={(size) => {
                if (size > 1) dispatch(setHierarchyPct(size));
              }}
              className="workspace-pane"
            >
              <TabContainer panelId="left" views={viewBodies} />
            </Panel>

            <PanelResizeHandle className="resize-handle resize-handle-h" />

            <Panel
              id="panel-center"
              order={2}
              minSize={30}
              className="workspace-pane"
            >
              <PanelGroup direction="vertical">
                <Panel
                  id="panel-viewport"
                  order={1}
                  minSize={20}
                  className="workspace-pane"
                >
                  <TabContainer panelId="center" views={viewBodies} />
                </Panel>

                <PanelResizeHandle className="resize-handle resize-handle-v" />

                <Panel
                  id="panel-bottom"
                  order={2}
                  ref={bottomPanelRef}
                  collapsible
                  collapsedSize={3}
                  defaultSize={
                    layout.consoleCollapsed ? 3 : layout.bottomPct
                  }
                  minSize={12}
                  maxSize={60}
                  onResize={(size) => {
                    if (size > 4) dispatch(setBottomPct(size));
                  }}
                  className="workspace-pane"
                >
                  <TabContainer panelId="bottom" views={viewBodies} />
                </Panel>
              </PanelGroup>
            </Panel>

            <PanelResizeHandle className="resize-handle resize-handle-h" />

            <Panel
              id="panel-right"
              order={3}
              ref={inspectorPanelRef}
              collapsible
              collapsedSize={0}
              defaultSize={layout.inspectorHidden ? 0 : layout.inspectorPct}
              minSize={14}
              maxSize={45}
              onResize={(size) => {
                if (size > 1) dispatch(setInspectorPct(size));
              }}
              className="workspace-pane"
            >
              <TabContainer panelId="right" views={viewBodies} />
            </Panel>
          </PanelGroup>
      </div>

      <SoundPlayer nowPlaying={nowPlaying} onLog={log} />

      <StatusBar
        summary={summary}
        meshesCount={meshes?.moby_assets.length ?? 0}
        instanceCount={instances.length}
        ufragCount={ufrags.length}
        meshes={meshes}
        loadPhase={loadPhase}
        meshLoadPhase={meshLoadPhase}
        error={error}
        cacheState={cacheState}
        cacheProgress={cacheProgress}
        onOpenCacheLibrary={() => setCacheLibraryOpen(true)}
      />

      <OpenLevelModal
        open={openLevelModalOpen}
        busy={busy}
        onClose={() => setOpenLevelModalOpen(false)}
        onOpen={(folder, opts) => handleOpen(folder, opts)}
      />

      <PsarcModal
        open={psarcModalOpen}
        onClose={() => setPsarcModalOpen(false)}
      />

      <AssetLookupModal
        open={assetLookupModalOpen}
        onClose={() => setAssetLookupModalOpen(false)}
      />

      <SettingsModal
        open={settingsModalOpen}
        onClose={() => setSettingsModalOpen(false)}
      />

      <GltfCharacterModal
        file={previewGltfFile}
        onClose={() => setPreviewGltfFile(null)}
        levelFolder={summary?.folder ?? null}
      />

      <CacheLibraryModal
        open={cacheLibraryOpen || previewAssetTuid !== null}
        onClose={() => {
          setCacheLibraryOpen(false);
          setPreviewAssetTuid(null);
          setCacheModalInitialPanel(null);
          setCacheModalInitialTextureId(null);
          setCacheModalInitialSoundKey(null);
        }}
        onSwitchMap={() => {
          setCacheLibraryOpen(false);
          setPreviewAssetTuid(null);
          setCacheModalInitialPanel(null);
          setCacheModalInitialTextureId(null);
          setCacheModalInitialSoundKey(null);
          setOpenLevelModalOpen(true);
        }}
        onCloseGame={() => {
          setCacheLibraryOpen(false);
          setPreviewAssetTuid(null);
          setCacheModalInitialPanel(null);
          setCacheModalInitialTextureId(null);
          setCacheModalInitialSoundKey(null);
          handleClose();
          setOpenLevelModalOpen(true);
        }}
        onOpenInWorkbench={(tuid, kind) => {
          openInWorkbench(tuid, kind);
          setCacheLibraryOpen(false);
          setPreviewAssetTuid(null);
          setCacheModalInitialPanel(null);
          setCacheModalInitialTextureId(null);
          setCacheModalInitialSoundKey(null);
        }}
        folder={summary?.folder ?? null}
        initialAssetTuid={previewAssetTuid}
        initialPanel={cacheModalInitialPanel}
        initialTextureId={cacheModalInitialTextureId}
        initialSoundKey={cacheModalInitialSoundKey}
        sounds={levelSounds}
        currentSkyboxTextureId={view.skyboxTextureId}
        cubemapDescriptor={cubemapDescriptor}
        onUseAsSkybox={(id) => {
          dispatch(setSkybox(id < 0 ? null : id));
          if (id >= 0) {
            setCacheModalInitialPanel("sky");
          }
        }}
        onRequestExtract={() => {
          if (!summary) return;
          console.log("[cache-modal] user requested extract", summary.folder);
          const channel = new Channel<CacheEvent>();
          let phaseTotals = { phase: "mobys" as const, total: 0 };
          channel.onmessage = (event) => {
            switch (event.type) {
              case "phase":
                phaseTotals = {
                  phase: event.phase,
                  total: event.total,
                } as never;
                setCacheProgress({
                  phase: event.phase,
                  current: 0,
                  total: event.total,
                });
                break;
              case "progress":
                setCacheProgress({
                  phase: phaseTotals.phase,
                  current: event.current,
                  total: phaseTotals.total,
                });
                break;
              case "done":
                setCacheProgress(null);
                cacheStatus(summary.folder).then(setCacheState).catch(() => {});
                readCachedManifest(summary.folder)
                  .then(setCacheManifest)
                  .catch(() => {});
                log("ok", `Cache built: ${event.entry_count} entries`);
                break;
              case "error":
                setCacheProgress(null);
                log("warn", `Cache extraction failed: ${event.message}`);
                break;
            }
          };
          reextractLevelCache(summary.folder, channel).catch((e) => {
            setCacheProgress(null);
            log("warn", `Cache extraction failed: ${e}`);
          });
        }}
      />

      <Modal
        open={cachePrompt !== null}
        dismissable={false}
        title={
          cachePrompt?.status.incomplete
            ? t("cachePrompt.titleIncomplete")
            : t("cachePrompt.titleFound")
        }
        subtitle={
          cachePrompt
            ? t("cachePrompt.subtitle", {
                mobys: cachePrompt.status.mobys,
                ties: cachePrompt.status.ties,
                textures: cachePrompt.status.textures,
              })
            : undefined
        }
        size="md"
        footer={
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button
              className="btn"
              onClick={() => resolveCachePrompt("use-cache")}
              disabled={cachePrompt?.status.incomplete}
              title={
                cachePrompt?.status.incomplete
                  ? t("cachePrompt.useCachedDisabledTitle")
                  : undefined
              }
            >
              {t("cachePrompt.useCached")}
            </button>
            <button
              className="btn btn-primary"
              onClick={() => resolveCachePrompt("force-reextract")}
            >
              {t("cachePrompt.reextract")}
            </button>
          </div>
        }
      >
        {cachePrompt?.status.incomplete ? (
          <>
            <p className="small" style={{ marginTop: 0, lineHeight: 1.5 }}>
              {t("cachePrompt.cacheBodyIncomplete")}
            </p>
            <p className="small dim" style={{ lineHeight: 1.5 }}>
              {t("cachePrompt.cacheBodyIncompleteHint")}
            </p>
          </>
        ) : (
          <>
            <p className="small dim" style={{ marginTop: 0, lineHeight: 1.5 }}>
              {t("cachePrompt.cacheBodyOk")}
            </p>
            <p className="small dim" style={{ lineHeight: 1.5 }}>
              {t("cachePrompt.cacheBodyOkHint")}
            </p>
          </>
        )}
        {cachePrompt?.status.stale && !cachePrompt.status.incomplete && (
          <p
            className="small"
            style={{
              marginTop: 8,
              color: "var(--warn-fg, #d97706)",
              lineHeight: 1.5,
            }}
          >
            ⚠ {t("cachePrompt.staleWarning")}
          </p>
        )}
      </Modal>

      <Modal
        open={loadPhase !== null}
        dismissable={false}
        title="Loading level"
        subtitle="Reading the level summary from disk"
        size="md"
      >
        <LoadProgress active={loadPhase} completed={completedPhases} />
        <p className="small dim" style={{ marginTop: 16, lineHeight: 1.5 }}>
          Heavy decoding may briefly freeze the window.{" "}
          <strong>Don't close the app</strong> — it keeps working in the
          background and will return as soon as the current phase finishes.
        </p>
      </Modal>

      <Modal
        open={meshLoadPhase !== null || cacheProgress !== null}
        dismissable={false}
        title={
          cacheProgress
            ? t("cacheModal.title")
            : t("loadModal.title")
        }
        subtitle={
          cacheProgress
            ? t("cacheModal.subtitle")
            : t("loadModal.subtitle")
        }
        size="md"
      >
        {meshLoadPhase && !cacheProgress && (
          <>
            <div className="cache-progress-phases">
              {(
                [
                  "layout",
                  "shaders",
                  "mobys",
                  "ties",
                  "ufrags",
                  "textures",
                ] as const
              ).map((p) => {
                const order = [
                  "layout",
                  "shaders",
                  "mobys",
                  "ties",
                  "ufrags",
                  "textures",
                ];
                const idx = order.indexOf(meshLoadPhase.phase);
                const myIdx = order.indexOf(p);
                const state =
                  myIdx < idx ? "done" : myIdx === idx ? "active" : "pending";
                return (
                  <div
                    key={p}
                    className={`cache-progress-phase cache-progress-phase-${state}`}
                  >
                    <span className="cache-progress-phase-dot" aria-hidden />
                    <span className="cache-progress-phase-label">
                      {t(`loadModal.phase_${p}`)}
                    </span>
                    {state === "active" && meshLoadPhase.total > 0 && (
                      <span className="mono small dim">
                        {meshLoadPhase.current.toLocaleString()} /{" "}
                        {meshLoadPhase.total.toLocaleString()}
                      </span>
                    )}
                    {state === "done" && (
                      <span className="cache-progress-check" aria-hidden>
                        ✓
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
            <div className="cache-progress-bar">
              <div
                className="cache-progress-bar-fill"
                style={{
                  width: `${
                    meshLoadPhase.total > 0
                      ? Math.min(
                          100,
                          Math.round(
                            (meshLoadPhase.current / meshLoadPhase.total) * 100,
                          ),
                        )
                      : 0
                  }%`,
                }}
              />
            </div>
          </>
        )}

        {cacheProgress && (
          <>
            <div className="cache-progress-phases">
              {(
                [
                  "mobys",
                  "ties",
                  "textures",
                  "normalmaps",
                  "materials",
                  "animations",
                ] as const
              ).map((p) => {
                // Backend emits real progress for mobys/ties/textures/
                // normalmaps/materials. The texture sub-phases run in
                // EMISSIONS → NORMALS → ALBEDOS order so albedo (the
                // visible-look channel) lands last. "animations" is
                // cosmetic — anims are decoded inline during the mobys
                // phase, so it flips done as soon as we've moved past mobys.
                const realOrder = [
                  "mobys",
                  "ties",
                  "textures",
                  "normalmaps",
                  "materials",
                ];
                const realIdx = realOrder.indexOf(cacheProgress.phase);
                const cosmetic = p === "animations";
                let state: "done" | "active" | "pending";
                if (cosmetic) {
                  state = realIdx >= realOrder.indexOf("ties") ? "done" : "pending";
                } else {
                  const myIdx = realOrder.indexOf(p);
                  state =
                    myIdx < realIdx ? "done" : myIdx === realIdx ? "active" : "pending";
                }
                return (
                  <div
                    key={p}
                    className={`cache-progress-phase cache-progress-phase-${state}`}
                  >
                    <span className="cache-progress-phase-dot" aria-hidden />
                    <span className="cache-progress-phase-label">
                      {t(`cacheModal.phase_${p}`)}
                    </span>
                    {state === "active" && (
                      <span className="mono small dim">
                        {cacheProgress.current.toLocaleString()} /{" "}
                        {cacheProgress.total.toLocaleString()}
                      </span>
                    )}
                    {state === "done" && (
                      <span className="cache-progress-check" aria-hidden>
                        ✓
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
            <div className="cache-progress-bar">
              <div
                className="cache-progress-bar-fill"
                style={{
                  width: `${
                    cacheProgress.total > 0
                      ? Math.min(
                          100,
                          Math.round(
                            (cacheProgress.current / cacheProgress.total) * 100,
                          ),
                        )
                      : 0
                  }%`,
                }}
              />
            </div>
          </>
        )}

        <p className="cache-progress-warning small">
          ⚠ {t("loadModal.warning")}
        </p>
        <p className="small dim" style={{ marginTop: 6, lineHeight: 1.5 }}>
          {cacheProgress ? t("cacheModal.hint") : t("loadModal.hint")}
        </p>
      </Modal>

      <AboutModal
        open={aboutModalOpen}
        onClose={() => setAboutModalOpen(false)}
        updater={updater}
      />

      <DocsModal
        open={docsModalOpen}
        onClose={() => setDocsModalOpen(false)}
      />

      <WhatsNewModal open={whatsNew.open} onClose={whatsNew.close} />

      <UpdateChecker state={updater} />

      <Modal
        
        open={
          exportState !== null &&
          exportState.phase !== "done" &&
          !exportState.cancelled
        }
        onClose={() => setExportState(null)}
        
        dismissable={
          exportState?.phase === "done" || exportState?.cancelled === true
        }
        title="Exporting selection"
        subtitle="Building scene → encoding glTF → writing to disk"
        size="md"
      >
        {exportState && <ExportProgress state={exportState} />}
      </Modal>
    </div>
  );
}
