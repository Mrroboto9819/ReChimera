import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { Bounds, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import {
  Bone,
  Download,
  Eye,
  EyeOff,
  Menu,
  Pause,
  Play,
  Square,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { readCachedBytes, writeBytes } from "../api";
import type { Instance } from "../api";
import { Button } from "../ui";

interface AssetWorkbenchProps {
  instance: Instance | null;
  cacheFolder: string | null;
  /** Direct asset override (e.g. driven by the "open in workbench" button
   *  from CacheLibraryModal). When set, takes precedence over `instance`
   *  so the workbench can show an asset that isn't currently selected in
   *  the level hierarchy. */
  overrideAsset?: { tuid: string; kind: "moby" | "tie" } | null;
}

interface LoadedGlb {
  scene: THREE.Group;
  animations: THREE.AnimationClip[];
  meshes: MeshEntry[];
  textures: TextureEntry[];
  dispose: () => void;
}

interface MeshEntry {
  uuid: string;
  name: string;
  vertexCount: number;
  material: THREE.Material | null;
}

interface TextureEntry {
  key: string;
  name: string;
  slot: string;
  texture: THREE.Texture;
}

type DrawerTab = "submeshes" | "textures" | "animations";

const MATERIAL_TEXTURE_SLOTS = [
  "map",
  "normalMap",
  "roughnessMap",
  "metalnessMap",
  "emissiveMap",
  "aoMap",
  "alphaMap",
  "bumpMap",
] as const;

const FPS = 30;

function disposeGltfScene(scene: THREE.Object3D): void {
  const seenMats = new Set<THREE.Material>();
  const seenTex = new Set<THREE.Texture>();
  scene.traverse((obj) => {
    const mesh = obj as THREE.Mesh;
    if (mesh.isMesh) {
      mesh.geometry?.dispose();
      const mats = Array.isArray(mesh.material)
        ? mesh.material
        : mesh.material
          ? [mesh.material]
          : [];
      for (const mat of mats) {
        if (seenMats.has(mat)) continue;
        seenMats.add(mat);
        for (const slot of MATERIAL_TEXTURE_SLOTS) {
          const t = (mat as unknown as Record<string, unknown>)[slot];
          if (t instanceof THREE.Texture && !seenTex.has(t)) {
            seenTex.add(t);
            t.dispose();
          }
        }
        mat.dispose();
      }
    }
  });
}

// Distinct tint for primitives whose material couldn't resolve any
// texture (the cache-extract step found the shader's referenced texture
// IDs but none of them shipped on this user's disk). Lets data gaps
// stand out from normal lighting instead of looking like flat gray
// engine output the user might mistake for a rendering bug.
const MISSING_TEXTURE_TINT = new THREE.Color(0xbb55ff);

function tintMissingTextureMaterials(scene: THREE.Object3D): void {
  const seen = new Set<THREE.Material>();
  scene.traverse((obj) => {
    const mesh = obj as THREE.Mesh;
    if (!mesh.isMesh) return;
    const mats = Array.isArray(mesh.material)
      ? mesh.material
      : mesh.material
        ? [mesh.material]
        : [];
    for (const mat of mats) {
      if (seen.has(mat)) continue;
      seen.add(mat);
      const std = mat as THREE.MeshStandardMaterial;
      const hasAlbedo = std.map != null;
      const hasNormal = std.normalMap != null;
      const hasEmissive = std.emissiveMap != null;
      if (!hasAlbedo && !hasNormal && !hasEmissive) {
        std.color = MISSING_TEXTURE_TINT.clone();
        std.roughness = 0.6;
        std.metalness = 0;
        std.needsUpdate = true;
      }
    }
  });
}

function collectMeshes(scene: THREE.Object3D): MeshEntry[] {
  const out: MeshEntry[] = [];
  scene.traverse((obj) => {
    const mesh = obj as THREE.Mesh;
    if (!mesh.isMesh) return;
    const geom = mesh.geometry;
    const vertexCount = geom?.attributes?.position?.count ?? 0;
    const material = Array.isArray(mesh.material)
      ? (mesh.material[0] ?? null)
      : (mesh.material ?? null);
    out.push({
      uuid: mesh.uuid,
      name: mesh.name || `mesh_${out.length}`,
      vertexCount,
      material,
    });
  });
  return out;
}

function collectTextures(scene: THREE.Object3D): TextureEntry[] {
  const seen = new Map<string, TextureEntry>();
  scene.traverse((obj) => {
    const mesh = obj as THREE.Mesh;
    if (!mesh.isMesh) return;
    const mats = Array.isArray(mesh.material)
      ? mesh.material
      : mesh.material
        ? [mesh.material]
        : [];
    for (const mat of mats) {
      for (const slot of MATERIAL_TEXTURE_SLOTS) {
        const t = (mat as unknown as Record<string, unknown>)[slot];
        if (!(t instanceof THREE.Texture)) continue;
        const key = `${t.uuid}:${slot}`;
        if (seen.has(key)) continue;
        seen.set(key, {
          key,
          name: t.name || mat.name || "(unnamed)",
          slot,
          texture: t,
        });
      }
    }
  });
  return Array.from(seen.values());
}

function textureToDataUrl(texture: THREE.Texture, maxDim = 96): string | null {
  const img = texture.image;
  if (!img || typeof img.width !== "number" || typeof img.height !== "number") {
    return null;
  }
  if (img.width === 0 || img.height === 0) return null;
  const scale = Math.min(1, maxDim / Math.max(img.width, img.height));
  const w = Math.max(1, Math.round(img.width * scale));
  const h = Math.max(1, Math.round(img.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  try {
    ctx.drawImage(img as CanvasImageSource, 0, 0, w, h);
    return canvas.toDataURL("image/png");
  } catch {
    return null;
  }
}

interface AnimRigProps {
  scene: THREE.Group;
  clip: THREE.AnimationClip | null;
  isPlaying: boolean;
  onFrame: (frame: number, duration: number) => void;
  /** Parent assigns a seek callback to this ref so the scrubber can
   *  jump action.time to a specific seconds offset without forcing the
   *  rig to be re-mounted (which would re-bind the mixer and reset
   *  everything). */
  seekRef?: React.MutableRefObject<((seconds: number) => void) | null>;
}

function AnimRig({ scene, clip, isPlaying, onFrame, seekRef }: AnimRigProps) {
  const mixerRef = useRef<THREE.AnimationMixer | null>(null);
  const actionRef = useRef<THREE.AnimationAction | null>(null);

  useEffect(() => {
    mixerRef.current = new THREE.AnimationMixer(scene);
    return () => {
      mixerRef.current?.stopAllAction();
      mixerRef.current = null;
      actionRef.current = null;
    };
  }, [scene]);

  useEffect(() => {
    const mixer = mixerRef.current;
    if (!mixer) return;
    mixer.stopAllAction();
    if (!clip) {
      actionRef.current = null;
      onFrame(0, 0);
      return;
    }
    const action = mixer.clipAction(clip);
    action.reset();
    action.play();
    action.paused = !isPlaying;
    actionRef.current = action;
    onFrame(0, clip.duration);
  }, [clip, isPlaying, onFrame]);

  useEffect(() => {
    const action = actionRef.current;
    if (!action) return;
    action.paused = !isPlaying;
  }, [isPlaying]);

  useEffect(() => {
    if (!seekRef) return;
    seekRef.current = (seconds: number) => {
      const mixer = mixerRef.current;
      const action = actionRef.current;
      if (!mixer || !action || !clip) return;
      const clamped = Math.max(0, Math.min(seconds, clip.duration));
      action.time = clamped;
      mixer.update(0);
      onFrame(Math.floor(clamped * FPS), clip.duration);
    };
    return () => {
      if (seekRef.current) seekRef.current = null;
    };
  }, [seekRef, clip, onFrame]);

  useFrame((_state, delta) => {
    const mixer = mixerRef.current;
    const action = actionRef.current;
    if (!mixer || !action || !clip) return;
    mixer.update(delta);
    const t = action.time;
    onFrame(Math.floor(t * FPS), clip.duration);
  });

  return null;
}

function SkeletonView({
  scene,
  visible,
}: {
  scene: THREE.Group;
  visible: boolean;
}) {
  const helper = useMemo(() => {
    const h = new THREE.SkeletonHelper(scene);
    const mat = h.material as THREE.LineBasicMaterial;
    mat.color.set(0x00ff88);
    mat.depthTest = false;
    mat.transparent = true;
    mat.opacity = 0.9;
    h.renderOrder = 999;
    return h;
  }, [scene]);
  useEffect(() => {
    return () => {
      helper.geometry.dispose();
      (helper.material as THREE.LineBasicMaterial).dispose();
    };
  }, [helper]);
  helper.visible = visible;
  return <primitive object={helper} />;
}

export function AssetWorkbench({
  instance,
  cacheFolder,
  overrideAsset,
}: AssetWorkbenchProps) {
  const { t } = useTranslation();
  const [loaded, setLoaded] = useState<LoadedGlb | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const [drawerOpen, setDrawerOpen] = useState(true);
  const [drawerTab, setDrawerTab] = useState<DrawerTab>("submeshes");
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [activeClipIndex, setActiveClipIndex] = useState<number | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [frame, setFrame] = useState(0);
  const [duration, setDuration] = useState(0);
  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const seekRef = useRef<((seconds: number) => void) | null>(null);
  const [showBones, setShowBones] = useState(false);
  const [hoveredUuid, setHoveredUuid] = useState<string | null>(null);

  const assetTuidHex = useMemo(() => {
    if (overrideAsset) return overrideAsset.tuid;
    if (!instance) return null;
    return instance.asset_tuid.split("#")[0] ?? null;
  }, [instance, overrideAsset]);

  const kind = overrideAsset
    ? overrideAsset.kind
    : instance?.kind === "moby" || instance?.kind === "tie"
      ? instance.kind
      : null;

  useEffect(() => {
    let cancelled = false;
    setLoaded(null);
    setError(null);
    setHidden(new Set());
    setActiveClipIndex(null);
    setIsPlaying(false);
    setFrame(0);
    setDuration(0);

    if (!cacheFolder || !assetTuidHex || !kind) {
      return;
    }

    const file = `${kind === "moby" ? "mobys" : "ties"}/${assetTuidHex}.glb`;
    setLoading(true);
    (async () => {
      try {
        const bytes = await readCachedBytes(cacheFolder, file);
        if (cancelled) return;
        const loader = new GLTFLoader();
        loader.parse(
          bytes,
          "",
          (gltf) => {
            if (cancelled) {
              disposeGltfScene(gltf.scene);
              return;
            }
            tintMissingTextureMaterials(gltf.scene);
            const meshes = collectMeshes(gltf.scene);
            const textures = collectTextures(gltf.scene);
            setLoaded({
              scene: gltf.scene as THREE.Group,
              animations: gltf.animations,
              meshes,
              textures,
              dispose: () => disposeGltfScene(gltf.scene),
            });
            setLoading(false);
          },
          (err) => {
            if (cancelled) return;
            setError(String(err));
            setLoading(false);
          },
        );
      } catch (e) {
        if (cancelled) return;
        setError(String(e));
        setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [cacheFolder, assetTuidHex, kind]);

  useEffect(() => {
    return () => {
      loaded?.dispose();
    };
  }, [loaded]);

  const setMeshHidden = useCallback((uuid: string, hidden: boolean) => {
    setHidden((prev) => {
      if (hidden === prev.has(uuid)) return prev;
      const next = new Set(prev);
      if (hidden) next.add(uuid);
      else next.delete(uuid);
      return next;
    });
  }, []);

  useEffect(() => {
    if (!loaded) return;
    loaded.scene.traverse((obj) => {
      if ((obj as THREE.Mesh).isMesh) {
        obj.visible = !hidden.has(obj.uuid);
      }
    });
  }, [loaded, hidden]);

  const hasBones = useMemo(() => {
    if (!loaded) return false;
    let found = false;
    loaded.scene.traverse((o) => {
      if ((o as THREE.Bone).isBone) found = true;
    });
    return found;
  }, [loaded]);

  useEffect(() => {
    if (!loaded || !hoveredUuid) return;
    const obj = loaded.scene.getObjectByProperty("uuid", hoveredUuid) as
      | THREE.Mesh
      | undefined;
    if (!obj || !(obj as THREE.Mesh).isMesh) return;
    const original = obj.material;
    const highlight = (m: THREE.Material): THREE.Material => {
      const c = m.clone();
      const std = c as THREE.MeshStandardMaterial;
      if (std.isMeshStandardMaterial) {
        std.emissive = new THREE.Color(0x33ddff);
        std.emissiveIntensity = 1.0;
      }
      return c;
    };
    const swapped = Array.isArray(original)
      ? original.map(highlight)
      : highlight(original);
    obj.material = swapped;
    return () => {
      obj.material = original;
      if (Array.isArray(swapped)) swapped.forEach((m) => m.dispose());
      else swapped.dispose();
    };
  }, [loaded, hoveredUuid]);

  const activeClip = useMemo(() => {
    if (!loaded || activeClipIndex == null) return null;
    return loaded.animations[activeClipIndex] ?? null;
  }, [loaded, activeClipIndex]);

  const handleFrame = useCallback((f: number, d: number) => {
    setFrame(f);
    if (d !== duration) setDuration(d);
  }, [duration]);

  const handleScrub = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!activeClip || !seekRef.current) return;
      const rect = e.currentTarget.getBoundingClientRect();
      const ratio = Math.max(
        0,
        Math.min(1, (e.clientX - rect.left) / rect.width),
      );
      seekRef.current(ratio * activeClip.duration);
    },
    [activeClip],
  );

  const handleExportGlb = useCallback(async () => {
    if (!cacheFolder || !assetTuidHex || !kind) return;
    setExportStatus("Saving…");
    try {
      const sourceFile = `${kind === "moby" ? "mobys" : "ties"}/${assetTuidHex}.glb`;
      const defaultName = `${assetTuidHex}.glb`;
      const path = await saveDialog({
        title: "Export current model as .glb",
        defaultPath: defaultName,
        filters: [{ name: "Binary glTF", extensions: ["glb"] }],
      });
      if (typeof path !== "string") {
        setExportStatus(null);
        return;
      }
      const bytes = await readCachedBytes(cacheFolder, sourceFile);
      await writeBytes(path, Array.from(new Uint8Array(bytes)));
      setExportStatus("Saved");
      setTimeout(() => setExportStatus(null), 1800);
    } catch (e) {
      setExportStatus(`Failed: ${e}`);
      setTimeout(() => setExportStatus(null), 3500);
    }
  }, [cacheFolder, assetTuidHex, kind]);

  if (!assetTuidHex || !cacheFolder || !kind) {
    return (
      <div className="aw-empty">
        <span className="dim small">{t("assetWorkbench.empty")}</span>
      </div>
    );
  }

  return (
    <div className="aw-root">
      <div className={`aw-drawer ${drawerOpen ? "open" : "closed"}`}>
        <button
          type="button"
          className="aw-burger"
          onClick={() => setDrawerOpen((v) => !v)}
          aria-label="Toggle layers"
          title="Toggle layers"
        >
          {drawerOpen ? <X size={14} /> : <Menu size={14} />}
        </button>
        {drawerOpen && (
          <>
            <div className="aw-drawer-tabs" role="tablist">
              <DrawerTabButton
                label={t("assetWorkbench.submeshes")}
                active={drawerTab === "submeshes"}
                onClick={() => setDrawerTab("submeshes")}
                count={loaded?.meshes.length ?? 0}
              />
              <DrawerTabButton
                label={t("assetWorkbench.textures")}
                active={drawerTab === "textures"}
                onClick={() => setDrawerTab("textures")}
                count={loaded?.textures.length ?? 0}
              />
              <DrawerTabButton
                label={t("assetWorkbench.animations")}
                active={drawerTab === "animations"}
                onClick={() => setDrawerTab("animations")}
                count={loaded?.animations.length ?? 0}
              />
            </div>
            <div className="aw-drawer-body">
              {loading && (
                <div className="aw-status dim small">
                  {t("assetWorkbench.loading")}
                </div>
              )}
              {error && (
                <div className="aw-status small" style={{ color: "#ff8b8b" }}>
                  {t("assetWorkbench.loadFailed")}: {error}
                </div>
              )}
              {!loading && !error && loaded && drawerTab === "submeshes" && (
                <SubmeshList
                  meshes={loaded.meshes}
                  hidden={hidden}
                  onSetHidden={setMeshHidden}
                  hoveredUuid={hoveredUuid}
                  onHover={setHoveredUuid}
                />
              )}
              {!loading && !error && loaded && drawerTab === "textures" && (
                <TextureList textures={loaded.textures} />
              )}
              {!loading && !error && loaded && drawerTab === "animations" && (
                <AnimationList
                  animations={loaded.animations}
                  activeIndex={activeClipIndex}
                  onPick={(i) => {
                    setActiveClipIndex(i);
                    setIsPlaying(true);
                  }}
                />
              )}
            </div>
          </>
        )}
      </div>

      <div className="aw-viewport">
        <Canvas
          camera={{ position: [3, 2, 3], fov: 40, near: 0.01, far: 10000 }}
          dpr={[1, 1.5]}
        >
          <color attach="background" args={["#0b0c0e"]} />
          <ambientLight intensity={0.55} />
          <directionalLight position={[5, 10, 7.5]} intensity={1.1} />
          <directionalLight position={[-5, -2, -5]} intensity={0.35} />
          {loaded && (
            <Bounds fit clip observe margin={1.2}>
              <primitive object={loaded.scene} />
            </Bounds>
          )}
          {loaded && hasBones && (
            <SkeletonView scene={loaded.scene} visible={showBones} />
          )}
          {loaded && (
            <AnimRig
              scene={loaded.scene}
              clip={activeClip}
              isPlaying={isPlaying}
              onFrame={handleFrame}
              seekRef={seekRef}
            />
          )}
          <OrbitControls makeDefault enableDamping dampingFactor={0.1} />
        </Canvas>

        <div className="aw-overlay mono small">
          <span className="aw-overlay-model">
            {instance?.name ?? assetTuidHex}
          </span>
          <span className="aw-overlay-clip dim">
            {activeClip ? activeClip.name : "rest pose"}
          </span>
        </div>

        {hasBones && (
          <div className="aw-view-controls">
            <button
              type="button"
              className={`aw-view-btn ${showBones ? "active" : ""}`}
              onClick={() => setShowBones((v) => !v)}
              aria-pressed={showBones}
              title={showBones ? "Hide skeleton" : "Show skeleton"}
              aria-label={showBones ? "Hide skeleton" : "Show skeleton"}
            >
              <Bone size={14} />
            </button>
          </div>
        )}


        <div className="aw-export-wrap">
          <Button
            variant="primary"
            icon={Download}
            onClick={handleExportGlb}
            disabled={!loaded || !cacheFolder}
            loading={exportStatus === "Saving…"}
            title="Export current model as .glb"
          >
            {exportStatus === "Saving…"
              ? "Exporting…"
              : exportStatus === "Saved"
                ? "Saved"
                : "Export .glb"}
          </Button>
        </div>

        <div className="aw-playback">
          <button
            type="button"
            className="aw-pb-btn"
            disabled={!activeClip}
            onClick={() => setIsPlaying((v) => !v)}
            title={isPlaying ? t("assetWorkbench.pause") : t("assetWorkbench.play")}
            aria-label={isPlaying ? "Pause" : "Play"}
          >
            {isPlaying ? <Pause size={12} /> : <Play size={12} />}
          </button>
          <button
            type="button"
            className="aw-pb-btn"
            disabled={!activeClip}
            onClick={() => {
              setIsPlaying(false);
              setActiveClipIndex(null);
            }}
            title={t("assetWorkbench.stop")}
            aria-label="Stop"
          >
            <Square size={10} />
          </button>
          <span className="aw-pb-clip small mono">
            {activeClip ? activeClip.name : "—"}
          </span>
          <div
            className={`aw-pb-timeline ${activeClip ? "" : "disabled"}`}
            onMouseDown={handleScrub}
            onMouseMove={(e) => {
              if (e.buttons === 1) handleScrub(e);
            }}
            role="slider"
            aria-label="Timeline scrubber"
            aria-valuemin={0}
            aria-valuemax={Math.max(0, Math.floor(duration * FPS))}
            aria-valuenow={frame}
          >
            <div
              className="aw-pb-timeline-fill"
              style={{
                width: duration > 0
                  ? `${Math.min(100, (frame / Math.max(1, duration * FPS)) * 100)}%`
                  : "0%",
              }}
            />
            {duration > 0 && Array.from({ length: Math.min(Math.floor(duration * FPS), 30) }).map((_, i, arr) => (
              <div
                key={i}
                className="aw-pb-timeline-tick"
                style={{ left: `${(i / Math.max(1, arr.length - 1)) * 100}%` }}
              />
            ))}
            <div
              className="aw-pb-timeline-cursor"
              style={{
                left: duration > 0
                  ? `${Math.min(100, (frame / Math.max(1, duration * FPS)) * 100)}%`
                  : "0%",
              }}
            />
          </div>
          <span className="aw-pb-meta small mono dim">
            {frame}
            {duration > 0 ? `/${Math.floor(duration * FPS)}` : ""}
          </span>
        </div>
      </div>
    </div>
  );
}

function DrawerTabButton({
  label,
  active,
  onClick,
  count,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  count: number;
}): ReactNode {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      className={`aw-drawer-tab ${active ? "active" : ""}`}
      onClick={onClick}
    >
      <span>{label}</span>
      <span className="aw-drawer-tab-count dim">{count}</span>
    </button>
  );
}

function SubmeshList({
  meshes,
  hidden,
  onSetHidden,
  hoveredUuid,
  onHover,
}: {
  meshes: MeshEntry[];
  hidden: Set<string>;
  onSetHidden: (uuid: string, hidden: boolean) => void;
  hoveredUuid: string | null;
  onHover: (uuid: string | null) => void;
}): ReactNode {
  const dragTargetRef = useRef<boolean | null>(null);

  useEffect(() => {
    const endDrag = () => {
      dragTargetRef.current = null;
    };
    window.addEventListener("mouseup", endDrag);
    window.addEventListener("mouseleave", endDrag);
    return () => {
      window.removeEventListener("mouseup", endDrag);
      window.removeEventListener("mouseleave", endDrag);
    };
  }, []);

  if (meshes.length === 0) {
    return <div className="aw-status small dim">—</div>;
  }

  const onEyeMouseDown = (uuid: string, isHidden: boolean) => (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const target = !isHidden;
    dragTargetRef.current = target;
    onSetHidden(uuid, target);
  };

  const onEyeMouseEnter = (uuid: string, isHidden: boolean) => () => {
    const target = dragTargetRef.current;
    if (target == null) return;
    if (isHidden === target) return;
    onSetHidden(uuid, target);
  };

  return (
    <ul className="aw-list" onDragStart={(e) => e.preventDefault()}>
      {meshes.map((m, i) => {
        const isHidden = hidden.has(m.uuid);
        return (
          <li
            key={m.uuid}
            className={`aw-row ${isHidden ? "hidden" : ""} ${m.uuid === hoveredUuid ? "is-hover" : ""}`}
            onMouseEnter={() => onHover(m.uuid)}
            onMouseLeave={() => onHover(null)}
          >
            <button
              type="button"
              className="aw-eye"
              onMouseDown={onEyeMouseDown(m.uuid, isHidden)}
              onMouseEnter={onEyeMouseEnter(m.uuid, isHidden)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSetHidden(m.uuid, !isHidden);
                }
              }}
              aria-label={isHidden ? "Show" : "Hide"}
              title={isHidden ? "Show (drag to paint)" : "Hide (drag to paint)"}
            >
              {isHidden ? <EyeOff size={12} /> : <Eye size={12} />}
            </button>
            <span className="aw-row-label small mono">
              [{i}] {m.name}
            </span>
            <span className="aw-row-meta small dim mono">
              {m.vertexCount}v
            </span>
          </li>
        );
      })}
    </ul>
  );
}

function TextureList({ textures }: { textures: TextureEntry[] }): ReactNode {
  if (textures.length === 0) {
    return <div className="aw-status small dim">—</div>;
  }
  return (
    <ul className="aw-list aw-tex-list">
      {textures.map((tex) => (
        <TextureRow key={tex.key} entry={tex} />
      ))}
    </ul>
  );
}

function TextureRow({ entry }: { entry: TextureEntry }): ReactNode {
  const [thumb, setThumb] = useState<string | null>(null);
  useEffect(() => {
    const img = entry.texture.image;
    if (!img) return;
    if ((img as HTMLImageElement).complete === false) {
      const handler = () => setThumb(textureToDataUrl(entry.texture));
      (img as HTMLImageElement).addEventListener("load", handler);
      return () => (img as HTMLImageElement).removeEventListener("load", handler);
    }
    setThumb(textureToDataUrl(entry.texture));
  }, [entry.texture]);

  return (
    <li className="aw-row aw-tex-row">
      <div className="aw-tex-thumb">
        {thumb ? (
          <img src={thumb} alt={entry.name} />
        ) : (
          <span className="dim small">…</span>
        )}
      </div>
      <div className="aw-tex-meta">
        <span className="aw-row-label small mono">{entry.name}</span>
        <span className="aw-row-meta small dim mono">{entry.slot}</span>
      </div>
    </li>
  );
}

function AnimationList({
  animations,
  activeIndex,
  onPick,
}: {
  animations: THREE.AnimationClip[];
  activeIndex: number | null;
  onPick: (index: number) => void;
}): ReactNode {
  const { t } = useTranslation();
  if (animations.length === 0) {
    return <div className="aw-status small dim">{t("assetWorkbench.noAnimations")}</div>;
  }
  return (
    <ul className="aw-list">
      {animations.map((clip, i) => {
        const active = i === activeIndex;
        return (
          <li
            key={`${clip.name}-${i}`}
            className={`aw-row aw-anim-row ${active ? "active" : ""}`}
          >
            <button
              type="button"
              className="aw-anim-row-btn"
              onClick={() => onPick(i)}
              aria-pressed={active}
              title={clip.name || `clip_${i}`}
            >
              <span className="aw-row-label small mono">{clip.name || `clip_${i}`}</span>
              <span className="aw-row-meta small dim mono">
                {clip.duration.toFixed(2)}s
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
