import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Canvas, useFrame } from "@react-three/fiber";
import { Bounds, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import type { GLTF } from "three/examples/jsm/loaders/GLTFLoader.js";
import { GLTFExporter } from "three/examples/jsm/exporters/GLTFExporter.js";
import { DDSLoader } from "three/examples/jsm/loaders/DDSLoader.js";
import { clone as skeletonAwareClone } from "three/examples/jsm/utils/SkeletonUtils.js";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import {
  fetchAnimsetClip,
  findGlbTextures,
  listAnimsetClips,
  readFileBytes,
  type AnimsetSummary,
  type DecodedClip,
  type GltfFile,
} from "../api";
import { Modal } from "./Modal";
import { Button, Checkbox } from "../ui";

interface GltfCharacterModalProps {
  
  file: GltfFile | null;
  onClose: () => void;
  



  levelFolder?: string | null;
}

type AnimSource = "file" | "level";










export function GltfCharacterModal({
  file,
  onClose,
  levelFolder,
}: GltfCharacterModalProps) {
  const open = file !== null;
  const [gltf, setGltf] = useState<GLTF | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [activeClipName, setActiveClipName] = useState<string | null>(null);
  const [showSkeleton, setShowSkeleton] = useState(false);
  
  
  const [animSource, setAnimSource] = useState<AnimSource>("file");
  
  
  const [levelAnimsets, setLevelAnimsets] = useState<AnimsetSummary[] | null>(null);
  const [levelAnimsetsError, setLevelAnimsetsError] = useState<string | null>(null);
  
  
  
  const [appliedRawClip, setAppliedRawClip] = useState<THREE.AnimationClip | null>(null);
  const [appliedRawClipName, setAppliedRawClipName] = useState<string | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  
  
  
  const [showAllAnimsets, setShowAllAnimsets] = useState(false);

  
  
  useEffect(() => {
    if (!open || !file) {
      setGltf(null);
      setActiveClipName(null);
      setLoadError(null);
      setAppliedRawClip(null);
      setAppliedRawClipName(null);
      setExportError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    setGltf(null);
    (async () => {
      try {
        
        
        
        
        const raw = await readFileBytes(file.path);
        const buf: ArrayBuffer =
          raw instanceof ArrayBuffer
            ? raw
            : ((raw as unknown as Uint8Array).buffer.slice(0) as ArrayBuffer);
        const loader = new GLTFLoader();
        
        
        
        
        loader.parse(
          buf,
          "",
          (loaded) => {
            if (cancelled) return;
            setGltf(loaded);
            // Auto-play the first animation if there is one.
            if (loaded.animations.length > 0) {
              setActiveClipName(loaded.animations[0]!.name);
            }
            setLoading(false);
          },
          (err) => {
            if (cancelled) return;
            setLoadError(`Parse failed: ${err}`);
            setLoading(false);
          },
        );
      } catch (e) {
        if (cancelled) return;
        setLoadError(`Read failed: ${e}`);
        setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, file]);

  // Load level animset list as soon as a level is open and the modal
  // mounts a file. Don't gate on the source dropdown — we want the
  // "matching animations for this character" filter ready before the
  
  
  useEffect(() => {
    if (!open || !levelFolder) return;
    if (levelAnimsets !== null) return; 
    let cancelled = false;
    setLevelAnimsetsError(null);
    listAnimsetClips(levelFolder)
      .then((list) => {
        if (cancelled) return;
        setLevelAnimsets(list);
      })
      .catch((e) => {
        if (cancelled) return;
        setLevelAnimsetsError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, levelFolder, levelAnimsets]);

  
  
  useEffect(() => {
    if (!open) setLevelAnimsets(null);
  }, [open]);

  
  
  
  useEffect(() => {
    if (!gltf || !levelFolder) return;
    if (gltf.animations.length === 0 && animSource === "file") {
      setAnimSource("level");
    }
  }, [gltf, levelFolder, animSource]);

  
  
  const [textureStatus, setTextureStatus] = useState<{
    requested: number;
    loaded: number;
  } | null>(null);

  
  
  
  
  
  
  
  
  
  
  useEffect(() => {
    if (!gltf || !levelFolder) {
      setTextureStatus(null);
      return;
    }
    let cancelled = false;
    const loader = new DDSLoader();
    const blobsToRevoke: string[] = [];
    const texturesToDispose: THREE.Texture[] = [];

    (async () => {
      
      const matsByName = new Map<string, THREE.MeshStandardMaterial[]>();
      gltf.scene.traverse((obj) => {
        if (!(obj as THREE.Mesh).isMesh) return;
        const mesh = obj as THREE.Mesh;
        const mats = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
        for (const m of mats) {
          if (!(m instanceof THREE.MeshStandardMaterial)) continue;
          if (!m.name) continue;
          let arr = matsByName.get(m.name);
          if (!arr) {
            arr = [];
            matsByName.set(m.name, arr);
          }
          arr.push(m);
        }
      });
      const names = Array.from(matsByName.keys());
      if (names.length === 0) return;
      setTextureStatus({ requested: names.length, loaded: 0 });

      
      let resolved: Awaited<ReturnType<typeof findGlbTextures>>;
      try {
        resolved = await findGlbTextures(levelFolder, names);
      } catch (e) {
        if (!cancelled) {
          
          console.warn("findGlbTextures failed:", e);
        }
        return;
      }
      if (cancelled) return;
      
      
      
      
      const withFiles = resolved.filter(
        (r) => r.albedo_path || r.normal_path || r.emissive_path,
      ).length;
      
      console.log("[findGlbTextures]", {
        materials: names.length,
        withFiles,
        firstResolved: resolved.slice(0, 3).map((r) => ({
          name: r.material_name,
          albedo: r.albedo_path?.split(/[\\/]/).pop(),
          normal: r.normal_path?.split(/[\\/]/).pop(),
          emissive: r.emissive_path?.split(/[\\/]/).pop(),
        })),
      });

      
      let loaded = 0;
      for (const r of resolved) {
        if (cancelled) return;
        const targets = matsByName.get(r.material_name) ?? [];
        if (targets.length === 0) continue;

        const channels: Array<["albedo_path" | "normal_path" | "emissive_path", string | null]> = [
          ["albedo_path", r.albedo_path],
          ["normal_path", r.normal_path],
          ["emissive_path", r.emissive_path],
        ];

        for (const [channel, path] of channels) {
          if (!path) continue;
          try {
            const ddsRaw = await readFileBytes(path);
            const ddsBuf: ArrayBuffer =
              ddsRaw instanceof ArrayBuffer
                ? ddsRaw
                : ((ddsRaw as unknown as Uint8Array).buffer.slice(0) as ArrayBuffer);
            if (cancelled) return;
            
            
            
            
            
            const parsed = loader.parse(ddsBuf, true);
            
            
            
            
            const fmt = parsed.format as THREE.CompressedPixelFormat;
            const tex = new THREE.CompressedTexture(
              parsed.mipmaps as ImageData[],
              parsed.width,
              parsed.height,
              fmt,
            );
            tex.mipmaps = parsed.mipmaps as ImageData[];
            tex.format = fmt;
            tex.minFilter = THREE.LinearMipmapLinearFilter;
            tex.magFilter = THREE.LinearFilter;
            tex.wrapS = THREE.RepeatWrapping;
            tex.wrapT = THREE.RepeatWrapping;
            tex.flipY = false;
            tex.colorSpace =
              channel === "albedo_path"
                ? THREE.SRGBColorSpace
                : THREE.NoColorSpace;
            tex.needsUpdate = true;
            texturesToDispose.push(tex);

            for (const m of targets) {
              if (channel === "albedo_path") m.map = tex;
              else if (channel === "normal_path") m.normalMap = tex;
              else if (channel === "emissive_path") {
                m.emissiveMap = tex;
                m.emissive = new THREE.Color(0xffffff);
                m.emissiveIntensity = 0.15;
              }
              m.needsUpdate = true;
            }
          } catch (e) {
            
            console.warn(`DDS load failed for ${path}:`, e);
          }
        }
        loaded++;
        setTextureStatus({ requested: names.length, loaded });
      }
    })();

    return () => {
      cancelled = true;
      for (const url of blobsToRevoke) URL.revokeObjectURL(url);
      
      
      
    };
  }, [gltf, levelFolder]);

  
  
  const characterStem = useMemo(() => {
    if (!file) return "";
    let stem = file.name.toLowerCase();
    stem = stem.replace(/\.(gltf|glb)$/i, "");
    stem = stem.replace(/\.entity$/i, "");
    return stem;
  }, [file]);

  // Filter level animsets by name relation to the character. Two
  // heuristics, in priority order:
  //   1. Exact substring match between clip name and stem (or vice versa)
  //   2. Token overlap — split on `_` / `-` / digits, count shared tokens
  // Returns matching clips first, with score for sorting.
  const { matchingAnimsets, otherAnimsets } = useMemo(() => {
    if (!levelAnimsets || !characterStem) {
      return { matchingAnimsets: [], otherAnimsets: levelAnimsets ?? [] };
    }
    const stemTokens = new Set(
      characterStem.split(/[_\-]+|\d+/).filter((t) => t.length >= 3),
    );
    type Scored = { clip: AnimsetSummary; score: number };
    const scored: Scored[] = levelAnimsets.map((clip) => {
      const name = clip.name.toLowerCase();
      // Direct substring wins big.
      if (name.includes(characterStem) || characterStem.includes(name)) {
        return { clip, score: 100 };
      }
      // Token overlap.
      const nameTokens = name.split(/[_\-]+|\d+/).filter((t) => t.length >= 3);
      let shared = 0;
      for (const t of nameTokens) if (stemTokens.has(t)) shared++;
      return { clip, score: shared };
    });
    const matching = scored
      .filter((s) => s.score > 0)
      .sort((a, b) => b.score - a.score)
      .map((s) => s.clip);
    const matchingTuids = new Set(matching.map((c) => c.tuid_hex));
    const others = levelAnimsets.filter((c) => !matchingTuids.has(c.tuid_hex));
    return { matchingAnimsets: matching, otherAnimsets: others };
  }, [levelAnimsets, characterStem]);

  // Find the loaded scene's first SkinnedMesh + its bones — needed for
  // remapping `bone_${i}` track names to whatever names the GLB uses.
  const sceneBones = useMemo<THREE.Bone[]>(() => {
    if (!gltf) return [];
    let found: THREE.Bone[] = [];
    gltf.scene.traverse((obj) => {
      if (found.length > 0) return;
      if (obj instanceof THREE.SkinnedMesh && obj.skeleton.bones.length > 0) {
        found = obj.skeleton.bones;
      }
    });
    return found;
  }, [gltf]);

  // User clicked a level animset → fetch decoded clip, remap bone names,
  // apply to the loaded scene's rig. The remap assumes IT writes bones
  // in the same index order as Insomniac's raw `.dat` skeleton table
  // (both ultimately read mobys.dat's bone array). If counts differ we
  // truncate to the smaller size — usually the GLB's skeleton has fewer
  // bones than face-only viseme clips, so missing tracks just no-op.
  const applyLevelClip = useCallback(
    async (clip: AnimsetSummary) => {
      if (!levelFolder) return;
      if (sceneBones.length === 0) return;
      try {
        // The GLB's bind-pose translations are already in the file's
        // unit (likely yards from IT's export). Pass position/scale = 1
        // so the raw clip's translation values stay in those same
        // units. Animation rotations are unitless (quaternions) so
        // unaffected.
        //
        // TODO: figure out the exact unit convention IT writes — until
        // then, raw clips that include bone TRANSLATIONS may look off
        // in the preview. Bone ROTATIONS (which dominate idle/walk
        // anims) work fine.
        const decoded: DecodedClip = await fetchAnimsetClip(
          levelFolder,
          clip.tuid_hex,
          0, // bind_pose_inverse_offset — passing 0 means positionScale=1
          0, // scale_shift — same
        );
        const aclip = buildClipForGlbBones(decoded, sceneBones);
        // Diagnostics: log clip + scene-bone state when applying. If
        // tracks=0 the bone-name remap failed; if tracks > 0 but mesh
        // doesn't deform, the issue is in the mixer/clone path.
        // eslint-disable-next-line no-console
        console.log("[applyLevelClip]", {
          clipName: clip.name,
          decodedBones: decoded.bones.length,
          sceneBones: sceneBones.length,
          firstFiveSceneBoneNames: sceneBones.slice(0, 5).map((b) => b.name),
          builtTracks: aclip.tracks.length,
          firstThreeTrackNames: aclip.tracks.slice(0, 3).map((t) => t.name),
          duration: aclip.duration,
        });
        setAppliedRawClip(aclip);
        setAppliedRawClipName(clip.name);
        // Clear file-source clip selection so only one plays at a time.
        setActiveClipName(null);
      } catch (e) {
        setLevelAnimsetsError(`Apply failed: ${e}`);
      }
    },
    [levelFolder, sceneBones],
  );

  // Export the loaded scene + the currently-applied clip(s) as a .glb
  // for Blender. Clips that came with the file (gltf.animations) AND
  // any raw clip we applied get shipped together so Blender's Action
  // editor sees both.
  const handleExport = useCallback(async () => {
    if (!gltf) return;
    setExportBusy(true);
    setExportError(null);
    try {
      const defaultName = file
        ? file.name.replace(/\.(gltf|glb)$/i, "") + ".glb"
        : "character.glb";
      const path = await save({
        title: "Export character with animations as .glb",
        defaultPath: defaultName,
        filters: [{ name: "Binary glTF", extensions: ["glb"] }],
      });
      if (!path || typeof path !== "string") {
        setExportBusy(false);
        return;
      }

      
      
      const animations: THREE.AnimationClip[] = [...gltf.animations];
      if (appliedRawClip) animations.push(appliedRawClip);

      const exporter = new GLTFExporter();
      const bytes = await new Promise<ArrayBuffer>((resolve, reject) => {
        exporter.parse(
          gltf.scene,
          (out) => {
            if (out instanceof ArrayBuffer) resolve(out);
            else reject(new Error("GLTFExporter returned JSON; expected binary"));
          },
          (err) => reject(err),
          {
            binary: true,
            includeCustomExtensions: false,
            embedImages: true,
            animations,
          },
        );
      });

      await invoke<void>("write_bytes", {
        path,
        bytes: Array.from(new Uint8Array(bytes)),
      });
    } catch (e) {
      setExportError(String(e));
    } finally {
      setExportBusy(false);
    }
  }, [gltf, file, appliedRawClip]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={file ? file.name : "GLTF preview"}
      subtitle={file ? `${(file.size_bytes / 1024).toFixed(1)} KB · ${file.extension.toUpperCase()}` : undefined}
      size="xl"
      bodyClassName="modal-body-flex"
      footer={
        <>
          <Button
            variant="primary"
            onClick={handleExport}
            disabled={!gltf}
            loading={exportBusy}
            title="Export the character + currently selected animations as .glb (one Action per clip when imported into Blender)"
          >
            {exportBusy ? "Exporting…" : "Export .glb"}
          </Button>
          <Button onClick={onClose}>Close</Button>
        </>
      }
    >
      <div className="character-preview-body">
        <div className="character-preview-canvas">
          {loading && (
            <div className="asset-preview-empty">
              <span className="dim small">Loading…</span>
            </div>
          )}
          {loadError && (
            <div className="asset-preview-empty">
              <span className="error small">{loadError}</span>
            </div>
          )}
          {!loading && !loadError && gltf && (
            <Canvas
              camera={{ position: [3, 2, 3], fov: 40, near: 0.01, far: 10000 }}
              dpr={[1, 1.5]}
            >
              <color attach="background" args={["#0b0c0e"]} />
              <ambientLight intensity={0.55} />
              <directionalLight position={[5, 10, 7.5]} intensity={1.1} />
              <directionalLight position={[-5, -2, -5]} intensity={0.35} />
              <Bounds fit clip observe margin={1.2}>
                <GltfScene
                  gltf={gltf}
                  activeClipName={activeClipName}
                  rawClip={appliedRawClip}
                  showSkeleton={showSkeleton}
                />
              </Bounds>
              <OrbitControls
                makeDefault
                enableDamping
                dampingFactor={0.1}
                autoRotate={activeClipName == null && appliedRawClip == null}
                autoRotateSpeed={0.6}
              />
            </Canvas>
          )}
        </div>

        <div className="character-preview-meta">
          {gltf && (
            <>
              <div className="inspector-section">
                <h4>View</h4>
                <Checkbox
                  className="anim-row"
                  checked={showSkeleton}
                  onCheckedChange={setShowSkeleton}
                  label={
                    <span className="anim-row-name">
                      Show skeleton (bones)
                    </span>
                  }
                />
              </div>

              <div className="inspector-section">
                <h4>Animations</h4>
                {



}
                <div className="anim-source-row">
                  <label htmlFor="anim-source-select">Source</label>
                  <select
                    id="anim-source-select"
                    value={animSource}
                    onChange={(e) => setAnimSource(e.target.value as AnimSource)}
                  >
                    <option value="file">Bundled in this file</option>
                    <option value="level" disabled={!levelFolder}>
                      Level animsets{!levelFolder ? " (no level open)" : ""}
                    </option>
                  </select>
                </div>

                {animSource === "file" && (
                  <>
                    {gltf.animations.length === 0 ? (
                      <p className="dim small">
                        No animation clips in this file. InsomniaToolset
                        usually splits character mesh and animations into
                        separate `.glb`s — switch the source to "Level
                        animsets" to load clips from the open level's
                        `animsets.dat`.
                      </p>
                    ) : (
                      <div className="anim-list">
                        {gltf.animations.map((clip) => (
                          <button
                            key={clip.name}
                            type="button"
                            className={`anim-row ${activeClipName === clip.name ? "active" : ""}`}
                            onClick={() => {
                              setActiveClipName(
                                activeClipName === clip.name ? null : clip.name,
                              );
                              setAppliedRawClip(null);
                              setAppliedRawClipName(null);
                            }}
                            title={`${clip.duration.toFixed(2)}s`}
                          >
                            <span className="anim-row-icon">
                              {activeClipName === clip.name ? "❚❚" : "▶"}
                            </span>
                            <span className="anim-row-name">{clip.name}</span>
                            <span className="anim-row-dur mono small">
                              {clip.duration.toFixed(2)}s
                            </span>
                          </button>
                        ))}
                      </div>
                    )}
                  </>
                )}

                {animSource === "level" && (
                  <>
                    {levelAnimsetsError && (
                      <p className="error small">{levelAnimsetsError}</p>
                    )}
                    {levelAnimsets === null && !levelAnimsetsError && (
                      <p className="dim small">Loading level animsets…</p>
                    )}
                    {levelAnimsets && levelAnimsets.length === 0 && (
                      <p className="dim small">
                        No animsets found in the level's `animsets.dat`.
                      </p>
                    )}
                    {

}
                    {matchingAnimsets.length > 0 && (
                      <>
                        <p className="dim small" style={{ marginTop: 4, marginBottom: 4 }}>
                          Matching this character ({matchingAnimsets.length})
                        </p>
                        <div className="anim-list">
                          {matchingAnimsets.map((c) => (
                            <AnimsetRow
                              key={c.tuid_hex}
                              clip={c}
                              active={appliedRawClipName === c.name}
                              onClick={() => applyLevelClip(c)}
                            />
                          ))}
                        </div>
                      </>
                    )}
                    {


}
                    {otherAnimsets.length > 0 && matchingAnimsets.length === 0 && !showAllAnimsets && (
                      <p className="dim small" style={{ marginTop: 10 }}>
                        No animations match this character. Click below
                        to browse the full level animset list.
                      </p>
                    )}
                    {otherAnimsets.length > 0 && (
                      <button
                        type="button"
                        className="btn btn-ghost"
                        style={{ marginTop: 10, width: "100%" }}
                        onClick={() => setShowAllAnimsets((v) => !v)}
                      >
                        {showAllAnimsets
                          ? `Hide other animations (${otherAnimsets.length})`
                          : `Show all other animations (${otherAnimsets.length})`}
                      </button>
                    )}
                    {otherAnimsets.length > 0 && showAllAnimsets && (
                      <>
                        <p className="dim small" style={{ marginTop: 10, marginBottom: 4 }}>
                          {matchingAnimsets.length > 0
                            ? `All other animations (${otherAnimsets.length})`
                            : `All animations (${otherAnimsets.length})`}
                        </p>
                        <div className="anim-list">
                          {otherAnimsets.map((c) => (
                            <AnimsetRow
                              key={c.tuid_hex}
                              clip={c}
                              active={appliedRawClipName === c.name}
                              onClick={() => applyLevelClip(c)}
                            />
                          ))}
                        </div>
                      </>
                    )}
                    {sceneBones.length === 0 && levelAnimsets && (
                      <p className="dim small" style={{ marginTop: 6 }}>
                        ⚠ This file has no skinned mesh — clips will load
                        but won't visibly animate the geometry.
                      </p>
                    )}
                  </>
                )}

                {exportError && (
                  <p className="error small" style={{ marginTop: 6 }}>
                    Export error: {exportError}
                  </p>
                )}
              </div>

              <div className="inspector-section">
                <h4>Stats</h4>
                <dl className="kv">
                  <dt>Animations</dt>
                  <dd>{gltf.animations.length}</dd>
                  <dt>Scenes</dt>
                  <dd>{gltf.scenes.length}</dd>
                  <dt>Cameras</dt>
                  <dd>{gltf.cameras?.length ?? 0}</dd>
                  {textureStatus && (
                    <>
                      <dt>Textures</dt>
                      <dd>
                        {textureStatus.loaded} / {textureStatus.requested} resolved
                      </dd>
                    </>
                  )}
                </dl>
              </div>

              <div className="inspector-section">
                <h4>File</h4>
                {file && (
                  <p className="mono small dim" style={{ wordBreak: "break-all" }}>
                    {file.path}
                  </p>
                )}
                <p className="dim small" style={{ marginTop: 6 }}>
                  Already a glTF — re-export from Blender if you need a
                  different format. The file on disk is unchanged.
                </p>
              </div>
            </>
          )}
        </div>
      </div>
    </Modal>
  );
}









function GltfScene({
  gltf,
  activeClipName,
  rawClip,
  showSkeleton,
}: {
  gltf: GLTF;
  activeClipName: string | null;
  

  rawClip: THREE.AnimationClip | null;
  showSkeleton: boolean;
}) {
  
  
  
  
  
  
  
  
  
  
  
  const scene = useMemo(() => skeletonAwareClone(gltf.scene), [gltf]);
  const mixerRef = useRef<THREE.AnimationMixer | null>(null);
  const actionRef = useRef<THREE.AnimationAction | null>(null);

  
  
  const skeletonRoot = useMemo(() => {
    let found: THREE.Object3D | null = null;
    scene.traverse((obj) => {
      if (found) return;
      if (obj instanceof THREE.SkinnedMesh && obj.skeleton.bones.length > 0) {
        const root = obj.skeleton.bones[0]!;
        
        
        let node: THREE.Object3D = root;
        while (node.parent && node.parent !== scene) {
          node = node.parent;
        }
        found = node;
      }
    });
    return found;
  }, [scene]);

  useEffect(() => {
    mixerRef.current = new THREE.AnimationMixer(scene);
    return () => {
      mixerRef.current?.stopAllAction();
      mixerRef.current?.uncacheRoot(scene);
      mixerRef.current = null;
    };
  }, [scene]);

  
  
  
  useEffect(() => {
    const mixer = mixerRef.current;
    if (!mixer) return;
    actionRef.current?.stop();
    actionRef.current = null;

    let clip: THREE.AnimationClip | null = null;
    if (rawClip) {
      clip = rawClip;
    } else if (activeClipName) {
      clip = gltf.animations.find((c) => c.name === activeClipName) ?? null;
    }
    if (!clip) return;
    const action = mixer.clipAction(clip);
    action.reset().play();
    actionRef.current = action;
  }, [activeClipName, gltf.animations, rawClip]);

  
  useFrame((_state, delta) => {
    mixerRef.current?.update(delta);
  });

  return (
    <>
      <primitive object={scene} />
      {showSkeleton && skeletonRoot && (
        <primitive object={new THREE.SkeletonHelper(skeletonRoot)} />
      )}
    </>
  );
}















function buildClipForGlbBones(
  decoded: DecodedClip,
  bones: THREE.Bone[],
): THREE.AnimationClip {
  const fps = decoded.frame_rate > 0 ? decoded.frame_rate : 30;
  const dt = 1 / fps;
  const tracks: THREE.KeyframeTrack[] = [];

  const animatedTimes: number[] = [];
  for (let i = 0; i < decoded.num_frames; i++) animatedTimes.push(i * dt);
  const staticTimes: number[] = [0];

  const bonesToUse = Math.min(decoded.bones.length, bones.length);
  for (let b = 0; b < bonesToUse; b++) {
    const decodedBone = decoded.bones[b];
    const targetName = bones[b]?.name;
    if (!decodedBone || !targetName) continue;

    if (decodedBone.rotations.length > 0) {
      const t = decodedBone.rotation_animated ? animatedTimes : staticTimes;
      tracks.push(
        new THREE.QuaternionKeyframeTrack(
          `${targetName}.quaternion`,
          t,
          decodedBone.rotations,
        ),
      );
    }
    if (decodedBone.translations.length > 0) {
      const t = decodedBone.translation_animated ? animatedTimes : staticTimes;
      tracks.push(
        new THREE.VectorKeyframeTrack(
          `${targetName}.position`,
          t,
          decodedBone.translations,
        ),
      );
    }
    if (decodedBone.scales.length > 0) {
      const t = decodedBone.scale_animated ? animatedTimes : staticTimes;
      tracks.push(
        new THREE.VectorKeyframeTrack(
          `${targetName}.scale`,
          t,
          decodedBone.scales,
        ),
      );
    }
  }

  const duration = decoded.num_frames > 0 ? (decoded.num_frames - 1) * dt : 0;
  return new THREE.AnimationClip(decoded.name || "clip", duration, tracks);
}



function AnimsetRow({
  clip,
  active,
  onClick,
}: {
  clip: AnimsetSummary;
  active: boolean;
  onClick: () => void;
}) {
  const dur = clip.frame_rate > 0 ? clip.num_frames / clip.frame_rate : 0;
  return (
    <button
      type="button"
      className={`anim-row ${active ? "active" : ""}`}
      onClick={onClick}
      title={`${clip.num_frames} frames @ ${clip.frame_rate.toFixed(0)}fps · ${clip.num_bones} bones${clip.looping ? " · looping" : ""}`}
    >
      <span className="anim-row-icon">{active ? "❚❚" : "▶"}</span>
      <span className="anim-row-name">{clip.name || "(unnamed)"}</span>
      <span className="anim-row-dur mono small">{dur.toFixed(2)}s</span>
    </button>
  );
}
