import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { Bounds, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { Menu, X } from "lucide-react";
import {
  decodeAnimsetClip,
  listAnimsets,
  readCachedBytes,
  type AnimsetSummary,
} from "../api";
import { Select, type SelectOption } from "../components/Select";
import { buildAnimationClip } from "../animClipBuilder";

interface GlbPreviewProps {
  folder: string;
  assetTuidHex: string;
  kind: "moby" | "tie";
  exportPicks?: ExportPicks;
  onExportPicksChange?: (picks: ExportPicks) => void;
}

export interface ExportPicks {
  byAnimset: Record<string, number[]>;
}

interface LoadedGlb {
  scene: THREE.Group;
  animations: THREE.AnimationClip[];
  dispose: () => void;
}

const MATERIAL_TEXTURE_SLOTS = [
  "map",
  "normalMap",
  "roughnessMap",
  "metalnessMap",
  "emissiveMap",
  "aoMap",
  "alphaMap",
  "bumpMap",
  "displacementMap",
  "envMap",
  "lightMap",
  "clearcoatMap",
  "clearcoatRoughnessMap",
  "clearcoatNormalMap",
  "sheenColorMap",
  "specularIntensityMap",
  "specularColorMap",
  "transmissionMap",
  "thicknessMap",
] as const;

function disposeGltfScene(scene: THREE.Object3D): void {
  const seenMaterials = new Set<THREE.Material>();
  const seenTextures = new Set<THREE.Texture>();
  scene.traverse((obj) => {
    const mesh = obj as THREE.Mesh & { skeleton?: THREE.Skeleton };
    if (mesh.isMesh) {
      mesh.geometry?.dispose();
      const mats = Array.isArray(mesh.material)
        ? mesh.material
        : mesh.material
          ? [mesh.material]
          : [];
      for (const mat of mats) {
        if (seenMaterials.has(mat)) continue;
        seenMaterials.add(mat);
        for (const slot of MATERIAL_TEXTURE_SLOTS) {
          const tex = (mat as unknown as Record<string, unknown>)[slot];
          if (tex instanceof THREE.Texture && !seenTextures.has(tex)) {
            seenTextures.add(tex);
            tex.dispose();
          }
        }
        mat.dispose();
      }
    }
    if ((obj as THREE.SkinnedMesh).isSkinnedMesh) {
      const skinned = obj as THREE.SkinnedMesh;
      skinned.skeleton?.dispose?.();
    }
  });
}

export function GlbPreview({
  folder,
  assetTuidHex,
  kind,
  exportPicks,
  onExportPicksChange,
}: GlbPreviewProps) {
  const [loaded, setLoaded] = useState<LoadedGlb | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [extraClips, setExtraClips] = useState<Map<string, THREE.AnimationClip>>(
    new Map(),
  );
  const [activeClipKey, setActiveClipKey] = useState<string>("");

  const [menuOpen, setMenuOpen] = useState(false);
  const [animsets, setAnimsets] = useState<AnimsetSummary[] | null>(null);
  const [animsetsError, setAnimsetsError] = useState<string | null>(null);
  const [loadingClipKey, setLoadingClipKey] = useState<string | null>(null);
  const [clipLoadError, setClipLoadError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [query, setQuery] = useState("");

  const localPicks = exportPicks ?? { byAnimset: {} };
  const setLocalPicks = (next: ExportPicks) => {
    onExportPicksChange?.(next);
  };

  useEffect(() => {
    let cancelled = false;
    setLoaded(null);
    setError(null);
    setExtraClips(new Map());
    setActiveClipKey("");
    setAnimsets(null);
    setAnimsetsError(null);
    setMenuOpen(false);
    setClipLoadError(null);
    setExpanded({});
    setQuery("");

    const file = `${kind === "moby" ? "mobys" : "ties"}/${assetTuidHex}.glb`;
    const probe9 = assetTuidHex === "0x000000000000079D";

    (async () => {
      try {
        const bytes = await readCachedBytes(folder, file);
        if (cancelled) return;
        if (probe9) {
          console.log("[probe-moby-0009] cached GLB byte length:", bytes.byteLength);
          const head = new Uint8Array(bytes.slice(0, 12));
          console.log(
            "[probe-moby-0009] glTF header bytes:",
            Array.from(head)
              .map((b) => b.toString(16).padStart(2, "0"))
              .join(" "),
          );
        }
        const loader = new GLTFLoader();
        loader.parse(
          bytes,
          "",
          (gltf) => {
            if (cancelled) return;
            if (probe9) {
              const j = gltf.parser?.json ?? {};
              console.log("[probe-moby] parsed GLB:", {
                nodes: (j.nodes ?? []).length,
                skins: (j.skins ?? []).length,
                meshes: (j.meshes ?? []).length,
                animations: (j.animations ?? []).length,
                accessors: (j.accessors ?? []).length,
              });
              const skin0 = j.skins?.[0];
              if (skin0) {
                console.log("[probe-moby] skin[0]:", {
                  joints: skin0.joints?.length,
                  ibm: skin0.inverseBindMatrices,
                  firstFiveJointNodes: skin0.joints?.slice(0, 5),
                });
              }
              let primStats = { skinned: 0, unskinned: 0, jointsOnly: 0 };
              for (const mesh of j.meshes ?? []) {
                for (const p of mesh.primitives ?? []) {
                  const hasJ = p.attributes?.JOINTS_0 != null;
                  const hasW = p.attributes?.WEIGHTS_0 != null;
                  if (hasJ && hasW) primStats.skinned++;
                  else if (hasJ && !hasW) primStats.jointsOnly++;
                  else primStats.unskinned++;
                }
              }
              console.log("[probe-moby] primitive skin status:", primStats);

              const anims = gltf.animations;
              console.log(
                "[probe-moby] embedded animations:",
                anims.length,
                "first 4 names:",
                anims.slice(0, 4).map((a) => a.name),
              );
              if (anims.length > 0) {
                const a0 = anims[0]!;
                const distinctNodes = new Set<string>();
                let translationTracks = 0;
                let rotationTracks = 0;
                let scaleTracks = 0;
                for (const t of a0.tracks) {
                  const node = t.name.split(".")[0]!;
                  distinctNodes.add(node);
                  if (t.name.endsWith(".position")) translationTracks++;
                  else if (t.name.endsWith(".quaternion")) rotationTracks++;
                  else if (t.name.endsWith(".scale")) scaleTracks++;
                }
                console.log(
                  `[probe-moby] anim[0] '${a0.name}' duration=${a0.duration.toFixed(3)}s tracks=${a0.tracks.length} distinct_nodes=${distinctNodes.size} rot=${rotationTracks} pos=${translationTracks} scale=${scaleTracks}`,
                );
                console.log(
                  "[probe-moby] anim[0] first 5 track names:",
                  a0.tracks.slice(0, 5).map((t) => t.name),
                );
              }

              // Walk scene to count skinned meshes and report bone resolution.
              let skinnedMeshCount = 0;
              let skinnedMeshBoneCount = 0;
              let nullBones = 0;
              gltf.scene.traverse((obj) => {
                const sm = obj as THREE.SkinnedMesh;
                if (sm.isSkinnedMesh) {
                  skinnedMeshCount++;
                  if (sm.skeleton) {
                    skinnedMeshBoneCount = sm.skeleton.bones.length;
                    for (const b of sm.skeleton.bones) {
                      if (!b) nullBones++;
                    }
                  }
                }
              });
              console.log(
                `[probe-moby] scene skinned-meshes=${skinnedMeshCount} skel_bones=${skinnedMeshBoneCount} null_bones=${nullBones}`,
              );
            }
            setLoaded({
              scene: gltf.scene,
              animations: gltf.animations,
              dispose: () => disposeGltfScene(gltf.scene),
            });
            if (gltf.animations.length > 0) {
              setActiveClipKey(`builtin:${gltf.animations[0]!.name}`);
            }
          },
          (err) => {
            if (probe9) {
              console.error("[probe-moby-0009] parse failed:", err);
            }
            if (!cancelled) {
              setError(`parse failed: ${err.message ?? err}`);
            }
          },
        );
      } catch (e) {
        if (probe9) {
          console.error("[probe-moby-0009] load failed:", e);
        }
        if (!cancelled) {
          setError(`load failed: ${e}`);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [folder, assetTuidHex, kind]);

  useEffect(() => {
    return () => {
      loaded?.dispose();
    };
  }, [loaded]);

  const ensureAnimsets = useCallback(async () => {
    if (animsets || animsetsError) return;
    try {
      const list = await listAnimsets(folder);
      setAnimsets(list);
    } catch (e) {
      setAnimsetsError(`${e}`);
    }
  }, [animsets, animsetsError, folder]);

  useEffect(() => {
    if (menuOpen) ensureAnimsets();
  }, [menuOpen, ensureAnimsets]);

  const ensureClipLoaded = async (
    animsetHash: string,
    clipIndex: number,
    clipName: string,
  ): Promise<{ key: string; clip: THREE.AnimationClip } | null> => {
    const key = `${animsetHash}:${clipIndex}`;
    const existing = extraClips.get(key);
    if (existing) return { key, clip: existing };
    setLoadingClipKey(key);
    setClipLoadError(null);
    try {
      const dto = await decodeAnimsetClip(folder, assetTuidHex, animsetHash, clipIndex);
      const clip = buildAnimationClip(dto);
      if (!clip.name) clip.name = clipName;
      setExtraClips((prev) => {
        const next = new Map(prev);
        next.set(key, clip);
        return next;
      });
      return { key, clip };
    } catch (e) {
      setClipLoadError(`Clip load failed: ${e}`);
      return null;
    } finally {
      setLoadingClipKey(null);
    }
  };

  const playClip = async (
    animsetHash: string,
    clipIndex: number,
    clipName: string,
  ) => {
    const loaded = await ensureClipLoaded(animsetHash, clipIndex, clipName);
    if (loaded) setActiveClipKey(`extra:${loaded.key}`);
  };

  const setExportPick = (animsetHash: string, clipIndex: number, on: boolean) => {
    const cur = new Set(localPicks.byAnimset[animsetHash] ?? []);
    if (on) cur.add(clipIndex);
    else cur.delete(clipIndex);
    const next: ExportPicks = {
      byAnimset: { ...localPicks.byAnimset },
    };
    if (cur.size === 0) delete next.byAnimset[animsetHash];
    else next.byAnimset[animsetHash] = [...cur].sort((a, b) => a - b);
    setLocalPicks(next);
  };

  const togglePick = async (
    animsetHash: string,
    clipIndex: number,
    clipName: string,
  ) => {
    const isPicked = (localPicks.byAnimset[animsetHash] ?? []).includes(clipIndex);
    if (isPicked) {
      setExportPick(animsetHash, clipIndex, false);
      const key = `${animsetHash}:${clipIndex}`;
      setExtraClips((prev) => {
        if (!prev.has(key)) return prev;
        const next = new Map(prev);
        next.delete(key);
        return next;
      });
      if (activeClipKey === `extra:${key}`) setActiveClipKey("");
    } else {
      const loaded = await ensureClipLoaded(animsetHash, clipIndex, clipName);
      if (loaded) {
        setExportPick(animsetHash, clipIndex, true);
      }
    }
  };

  const setAllForAnimset = (animsetHash: string, count: number, on: boolean) => {
    const next: ExportPicks = { byAnimset: { ...localPicks.byAnimset } };
    if (on && count > 0) {
      next.byAnimset[animsetHash] = Array.from({ length: count }, (_, i) => i);
    } else {
      delete next.byAnimset[animsetHash];
      setExtraClips((prev) => {
        const m = new Map(prev);
        for (const k of Array.from(m.keys())) {
          if (k.startsWith(`${animsetHash}:`)) m.delete(k);
        }
        return m;
      });
      if (activeClipKey.startsWith(`extra:${animsetHash}:`)) setActiveClipKey("");
    }
    setLocalPicks(next);
  };

  const setAllForAllAnimsets = (on: boolean) => {
    if (!animsets) return;
    if (on) {
      const next: ExportPicks = { byAnimset: { ...localPicks.byAnimset } };
      for (const a of animsets) {
        if (a.clips.length === 0) continue;
        next.byAnimset[a.hash] = Array.from({ length: a.clips.length }, (_, i) => i);
      }
      setLocalPicks(next);
    } else {
      setLocalPicks({ byAnimset: {} });
      setExtraClips(new Map());
      if (activeClipKey.startsWith("extra:")) setActiveClipKey("");
    }
  };

  const totalPicks = useMemo(
    () =>
      Object.values(localPicks.byAnimset).reduce(
        (acc, arr) => acc + arr.length,
        0,
      ),
    [localPicks],
  );

  const trimmedQuery = query.trim().toLowerCase();
  const visibleAnimsets = useMemo(() => {
    if (!animsets) return null;
    if (!trimmedQuery) {
      return animsets.map((a) => ({
        animset: a,
        visibleClipIndices: a.clips.map((_, i) => i),
        hashHit: false,
      }));
    }
    const out: {
      animset: AnimsetSummary;
      visibleClipIndices: number[];
      hashHit: boolean;
    }[] = [];
    for (const a of animsets) {
      const hashHit = a.hash.toLowerCase().includes(trimmedQuery);
      let visibleClipIndices: number[];
      if (hashHit) {
        visibleClipIndices = a.clips.map((_, i) => i);
      } else {
        visibleClipIndices = [];
        a.clips.forEach((c, i) => {
          if (c.name.toLowerCase().includes(trimmedQuery)) {
            visibleClipIndices.push(i);
          }
        });
      }
      if (!hashHit && visibleClipIndices.length === 0) continue;
      out.push({ animset: a, visibleClipIndices, hashHit });
    }
    return out;
  }, [animsets, trimmedQuery]);

  if (error) {
    return (
      <div className="asset-preview-empty">
        <span className="dim small">GLB load error: {error}</span>
      </div>
    );
  }
  if (!loaded) {
    return (
      <div className="asset-preview-empty">
        <span className="dim small">Loading GLB…</span>
      </div>
    );
  }

  const clipOptions: SelectOption[] = [
    { value: "", label: "— rest pose —" },
    ...loaded.animations.map<SelectOption>((c) => ({
      value: `builtin:${c.name}`,
      label: c.name,
      hint: `${c.duration.toFixed(2)}s · default`,
    })),
    ...Array.from(extraClips.entries()).map<SelectOption>(([key, c]) => ({
      value: `extra:${key}`,
      label: c.name,
      hint: `${c.duration.toFixed(2)}s · added`,
    })),
  ];

  const activeClip: THREE.AnimationClip | null = (() => {
    if (!activeClipKey) return null;
    if (activeClipKey.startsWith("builtin:")) {
      const name = activeClipKey.slice("builtin:".length);
      return loaded.animations.find((c) => c.name === name) ?? null;
    }
    if (activeClipKey.startsWith("extra:")) {
      return extraClips.get(activeClipKey.slice("extra:".length)) ?? null;
    }
    return null;
  })();

  return (
    <div className="glb-preview-stack">
      <div className="glb-preview-controls">
        <label className="dim small">Animation</label>
        <Select
          value={activeClipKey}
          onChange={setActiveClipKey}
          ariaLabel="Animation"
          className="glb-preview-select"
          options={clipOptions}
        />
        <button
          type="button"
          className={`glb-preview-burger${menuOpen ? " is-open" : ""}`}
          onClick={() => setMenuOpen((v) => !v)}
          aria-label={menuOpen ? "Close animation list" : "Open animation list"}
          title={
            totalPicks > 0
              ? `${totalPicks} clip${totalPicks === 1 ? "" : "s"} marked for export`
              : "All level animations"
          }
        >
          {menuOpen ? <X size={16} /> : <Menu size={16} />}
          {totalPicks > 0 && !menuOpen && (
            <span className="glb-preview-burger-badge">{totalPicks}</span>
          )}
        </button>
      </div>

      <div className="glb-preview-canvas-wrap">
        <Canvas
          camera={{ position: [3, 2, 3], fov: 40, near: 0.01, far: 10000 }}
          dpr={[1, 1.5]}
        >
          <color attach="background" args={["#0b0c0e"]} />
          <ambientLight intensity={0.55} />
          <directionalLight position={[5, 10, 7.5]} intensity={1.1} />
          <directionalLight position={[-5, -2, -5]} intensity={0.35} />

          <Bounds fit clip observe margin={1.2}>
            <GlbScene scene={loaded.scene} clip={activeClip} />
          </Bounds>

          <OrbitControls
            makeDefault
            enableDamping
            dampingFactor={0.1}
          />
        </Canvas>

        {menuOpen && (
          <aside className="glb-preview-menu">
            <header className="glb-preview-menu-header">
              <strong>Animations</strong>
              <span className="dim small">
                {totalPicks} marked for export
              </span>
            </header>
            <p className="dim small glb-preview-menu-hint">
              Play any animation on this skeleton, or check the box to bake it
              into the export GLB as an Action.
            </p>
            {animsets && animsets.length > 0 && (
              <div className="glb-preview-menu-search">
                <input
                  type="search"
                  className="glb-preview-menu-search-input"
                  placeholder="Filter animations…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  aria-label="Filter animations"
                />
                {query && (
                  <button
                    type="button"
                    className="glb-preview-menu-search-clear"
                    onClick={() => setQuery("")}
                    aria-label="Clear filter"
                    title="Clear filter"
                  >
                    ✕
                  </button>
                )}
              </div>
            )}
            {clipLoadError && (
              <p className="warn-text small">{clipLoadError}</p>
            )}
            {animsetsError && (
              <p className="warn-text small">
                Failed to list animsets: {animsetsError}
              </p>
            )}
            {!animsets && !animsetsError && (
              <p className="dim small">Loading animsets…</p>
            )}
            {animsets && visibleAnimsets && (
              <>
                {(() => {
                  const totalAvailable = animsets.reduce(
                    (acc, a) => acc + a.clips.length,
                    0,
                  );
                  const allChecked =
                    totalAvailable > 0 && totalPicks === totalAvailable;
                  const someChecked = totalPicks > 0 && !allChecked;
                  const allExpanded =
                    animsets.length > 0 &&
                    animsets.every((a) => expanded[a.hash]);
                  const visibleClipCount = visibleAnimsets.reduce(
                    (acc, v) => acc + v.visibleClipIndices.length,
                    0,
                  );
                  return (
                    <div className="glb-preview-menu-allrow">
                      <TriStateCheckbox
                        checked={allChecked}
                        indeterminate={someChecked}
                        onToggle={(next) => setAllForAllAnimsets(next)}
                        ariaLabel="Select all animations"
                      />
                      <span className="small glb-preview-menu-allrow-label">
                        <strong>All animsets</strong>
                        <span className="dim">
                          {" "}· {totalPicks} / {totalAvailable}
                          {trimmedQuery && (
                            <> · {visibleClipCount} match{visibleClipCount === 1 ? "" : "es"}</>
                          )}
                        </span>
                      </span>
                      <button
                        type="button"
                        className="btn small"
                        onClick={() => {
                          if (allExpanded) {
                            setExpanded({});
                          } else {
                            const next: Record<string, boolean> = {};
                            for (const a of animsets) next[a.hash] = true;
                            setExpanded(next);
                          }
                        }}
                      >
                        {allExpanded ? "Collapse all" : "Expand all"}
                      </button>
                    </div>
                  );
                })()}
                {visibleAnimsets.length === 0 && trimmedQuery && (
                  <p className="dim small glb-preview-menu-empty">
                    No animations match "{query.trim()}".
                  </p>
                )}
                <ul className="glb-preview-menu-list">
                {visibleAnimsets.map(({ animset: a, visibleClipIndices }) => {
                  const picked = localPicks.byAnimset[a.hash] ?? [];
                  const allInPicked =
                    a.clips.length > 0 && picked.length === a.clips.length;
                  const partial =
                    picked.length > 0 && picked.length < a.clips.length;
                  const isOpen = trimmedQuery
                    ? true
                    : !!expanded[a.hash];
                  return (
                  <li key={a.hash} className="glb-preview-menu-animset">
                    <div className="glb-preview-menu-animset-header mono small">
                      <button
                        type="button"
                        className="glb-preview-menu-chevron"
                        onClick={() =>
                          setExpanded((p) => ({ ...p, [a.hash]: !isOpen }))
                        }
                        aria-label={isOpen ? "Collapse" : "Expand"}
                        aria-expanded={isOpen}
                        disabled={!!trimmedQuery}
                      >
                        {isOpen ? "▾" : "▸"}
                      </button>
                      <TriStateCheckbox
                        checked={allInPicked}
                        indeterminate={partial}
                        onToggle={(next) =>
                          setAllForAnimset(a.hash, a.clips.length, next)
                        }
                        ariaLabel={`Select all in ${a.hash}`}
                      />
                      <span className="glb-preview-menu-animset-label">
                        {a.hash}
                        <span className="dim">
                          {" "}· {picked.length} / {a.clips.length}
                          {trimmedQuery &&
                            visibleClipIndices.length !== a.clips.length && (
                              <> · {visibleClipIndices.length} shown</>
                            )}
                        </span>
                      </span>
                    </div>
                    {isOpen && (
                    <ul className="glb-preview-menu-clips">
                      {visibleClipIndices.length === 0 && (
                        <li className="dim small">no clips</li>
                      )}
                      {visibleClipIndices.map((i) => {
                        const c = a.clips[i]!;
                        const key = `${a.hash}:${i}`;
                        const checked =
                          (localPicks.byAnimset[a.hash] ?? []).includes(i);
                        const isPlaying = activeClipKey === `extra:${key}`;
                        const loading = loadingClipKey === key;
                        return (
                          <li
                            key={i}
                            className={`glb-preview-menu-clip${isPlaying ? " is-playing" : ""}`}
                          >
                            <button
                              type="button"
                              className="glb-preview-menu-clip-play-btn"
                              onClick={() => playClip(a.hash, i, c.name)}
                              disabled={loading}
                              aria-label={`Play ${c.name}`}
                              title="Play in preview"
                            >
                              ▶
                            </button>
                            <label className="glb-preview-menu-clip-check">
                              <input
                                type="checkbox"
                                checked={checked}
                                onChange={() => togglePick(a.hash, i, c.name)}
                              />
                            </label>
                            <span className="glb-preview-menu-clip-info">
                              <span className="mono small">{c.name}</span>
                              <span className="dim small">
                                {c.num_frames}f · {c.frame_rate.toFixed(0)}fps
                                {loading ? " · loading…" : ""}
                              </span>
                            </span>
                          </li>
                        );
                      })}
                    </ul>
                    )}
                  </li>
                  );
                })}
                </ul>
              </>
            )}
          </aside>
        )}
      </div>
    </div>
  );
}

function TriStateCheckbox({
  checked,
  indeterminate,
  onToggle,
  ariaLabel,
}: {
  checked: boolean;
  indeterminate: boolean;
  onToggle: (next: boolean) => void;
  ariaLabel?: string;
}) {
  const ref = useRef<HTMLInputElement | null>(null);
  useLayoutEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return (
    <input
      ref={ref}
      type="checkbox"
      aria-label={ariaLabel}
      checked={checked}
      onChange={() => onToggle(!checked)}
    />
  );
}

function GlbScene({
  scene,
  clip,
}: {
  scene: THREE.Group;
  clip: THREE.AnimationClip | null;
}) {
  const mixer = useMemo(() => new THREE.AnimationMixer(scene), [scene]);
  const skeletonHelper = useMemo(() => {
    const helper = new THREE.SkeletonHelper(scene);
    const mat = helper.material as THREE.LineBasicMaterial;
    mat.color.set(0x00ff88);
    mat.depthTest = false;
    mat.transparent = true;
    mat.opacity = 0.9;
    helper.renderOrder = 999;
    return helper;
  }, [scene]);
  const actionRef = useRef<THREE.AnimationAction | null>(null);

  useEffect(() => {
    if (actionRef.current) {
      actionRef.current.stop();
      actionRef.current = null;
    }
    mixer.stopAllAction();
    if (!clip) return;
    const action = mixer.clipAction(clip);
    action.reset();
    action.setLoop(THREE.LoopRepeat, Infinity);
    action.play();
    actionRef.current = action;

    if (clip.name && clip.name.length > 0) {
      let bound = 0;
      let unbound = 0;
      for (const t of clip.tracks) {
        const targetName = t.name.split(".")[0]!;
        const found = scene.getObjectByName(targetName);
        if (found) bound++;
        else unbound++;
      }
      if (unbound > 0) {
        console.warn(
          `[probe-anim] clip '${clip.name}' has ${unbound}/${clip.tracks.length} tracks pointing at nodes that don't exist in the scene`,
        );
      } else {
        console.log(
          `[probe-anim] clip '${clip.name}' bound ${bound}/${clip.tracks.length} tracks (all targets resolved)`,
        );
      }
    }
  }, [mixer, clip, scene]);

  useFrame((_, delta) => {
    mixer.update(delta);
  });

  useEffect(() => {
    return () => {
      mixer.stopAllAction();
      mixer.uncacheRoot(scene);
      skeletonHelper.geometry.dispose();
      (skeletonHelper.material as THREE.LineBasicMaterial).dispose();
    };
  }, [mixer, scene, skeletonHelper]);

  return (
    <>
      <primitive object={scene} />
      <primitive object={skeletonHelper} />
    </>
  );
}
