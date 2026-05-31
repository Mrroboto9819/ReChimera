import * as THREE from "three";
import { GLTFExporter } from "three/examples/jsm/exporters/GLTFExporter.js";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import {
  decodeMeshGeom,
  fetchAnimsetClip,
  type AssetMeshes,
  type Instance,
  type LevelMeshes,
  type TextureBlobMap,
} from "./api";
import {
  buildAnimationClipFromDecoded,
  buildSkinnedAsset,
  isSkinnedAsset,
} from "./skinning";


export type ExportPhase =
  | "preparing"
  | "decoding-textures"
  | "encoding"
  | "writing"
  | "done";

export interface ExportProgressState {
  phase: ExportPhase;
  
  label: string;
  
  fraction: number;
  
  detail?: string;
  

  cancelled?: boolean;
}

export interface ExportResult {
  
  path: string;
  bytes: number;
}


function sanitizeFilename(s: string): string {
  
  return (
    s
      .replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")
      .replace(/\s+/g, "_")
      .replace(/^\.+/, "")
      .slice(0, 80) || "asset"
  );
}













export async function pickGlbExportPath(
  selectedInstances: Instance[],
): Promise<string | null> {
  let defaultName: string;
  if (selectedInstances.length === 0) {
    defaultName = "rechimera-0-objects.glb";
  } else if (selectedInstances.length === 1) {
    const inst = selectedInstances[0]!;
    const stem = sanitizeFilename(inst.name || inst.tuid.split("#")[0] || "asset");
    defaultName = `${stem}.glb`;
  } else {
    const first = selectedInstances[0]!;
    const stem = sanitizeFilename(first.name || first.tuid.split("#")[0] || "asset");
    defaultName = `${stem}_+${selectedInstances.length - 1}.glb`;
  }

  const path = await save({
    title: "Export selection as glTF binary (.glb)",
    defaultPath: defaultName,
    filters: [{ name: "Binary glTF", extensions: ["glb"] }],
  });
  return typeof path === "string" ? path : null;
}













export async function exportToGlb(
  selectedIds: Set<string>,
  instances: Instance[],
  meshes: LevelMeshes | null,
  


  textureBlobs: TextureBlobMap | null,
  path: string,
  onProgress?: (s: ExportProgressState) => void,
  levelFolder?: string | null,
  




  overrideAnimsetHash?: string | null,
): Promise<ExportResult> {
  const emit = (s: ExportProgressState) => onProgress?.(s);

  if (selectedIds.size === 0 || !meshes) {
    throw new Error("Nothing selected to export");
  }
  const selectedInstances = instances.filter((i) => selectedIds.has(i.tuid));
  if (selectedInstances.length === 0) {
    throw new Error("No matching instances in selection");
  }

  
  emit({
    phase: "preparing",
    label: "Building scene from selection",
    fraction: 0.05,
    detail: path,
  });
  await yieldToBrowser();

  const assetLib = new Map<string, AssetMeshes>();
  for (const a of meshes.moby_assets) assetLib.set(a.asset_tuid, a);
  for (const a of meshes.tie_assets) assetLib.set(a.asset_tuid, a);

  const root = new THREE.Group();
  root.name = `ReChimera-export-${selectedInstances.length}`;

  
  const neededAlbedos = new Set<number>();
  
  
  
  
  const animationClips: THREE.AnimationClip[] = [];
  
  
  const skinnedRigsToDispose: Array<{ dispose: () => void }> = [];

  for (let idx = 0; idx < selectedInstances.length; idx++) {
    const inst = selectedInstances[idx]!;
    const asset = assetLib.get(inst.asset_tuid);
    if (!asset) continue;

    const node = new THREE.Group();
    node.name = inst.name || inst.tuid;
    node.position.set(inst.position[0]!, inst.position[1]!, inst.position[2]!);
    node.quaternion.set(
      inst.quaternion[0]!,
      inst.quaternion[1]!,
      inst.quaternion[2]!,
      inst.quaternion[3]!,
    );
    node.scale.set(inst.scale[0]!, inst.scale[1]!, inst.scale[2]!);
    node.userData = {
      tuid: inst.tuid,
      asset_tuid: inst.asset_tuid,
      kind: inst.kind,
    };

    if (isSkinnedAsset(asset)) {
      
      
      
      
      const built = buildSkinnedAsset(asset);
      if (built) {
        skinnedRigsToDispose.push(built);
        node.add(built.root);
        for (const s of asset.submeshes) {
          if (s.albedo_id != null) neededAlbedos.add(s.albedo_id);
          if (s.normal_id != null) neededAlbedos.add(s.normal_id);
          if (s.emissive_id != null) neededAlbedos.add(s.emissive_id);
        }

        
        
        
        const targetHash = overrideAnimsetHash ?? asset.animset_hash;
        if (targetHash && levelFolder) {
          try {
            const decoded = await fetchAnimsetClip(
              levelFolder,
              targetHash,
              asset.bind_pose_inverse_offset ?? 0,
              asset.skeleton?.scale_shift ?? 0,
            );
            const aclip = buildAnimationClipFromDecoded(
              decoded,
              built.bones.length,
            );
            if (aclip.tracks.length > 0) {
              
              
              aclip.name = `${inst.name || inst.tuid}_${decoded.name || "clip"}`;
              animationClips.push(aclip);
              
              
              
              const sm = built.skinnedMeshes[0];
              if (sm) sm.animations = [aclip];
            }
          } catch (err) {
            
            console.warn(`Animset fetch failed for ${targetHash}:`, err);
          }
        }
      }
    } else {
      
      for (let i = 0; i < asset.submeshes.length; i++) {
        const s = asset.submeshes[i]!;
        const decoded = decodeMeshGeom(s);
        const geom = new THREE.BufferGeometry();
        geom.setAttribute("position", new THREE.BufferAttribute(decoded.positions, 3));
        if (decoded.uvs.length > 0) {
          geom.setAttribute("uv", new THREE.BufferAttribute(decoded.uvs, 2));
        }
        geom.setIndex(new THREE.BufferAttribute(decoded.indices, 1));

        const material = new THREE.MeshStandardMaterial({
          color: 0xffffff,
          roughness: 0.85,
          metalness: 0,
          emissive: 0x000000,
          emissiveIntensity: 0,
          name: `slots_a${s.albedo_id ?? "_"}_n${s.normal_id ?? "_"}_e${s.emissive_id ?? "_"}`,
        });
        if (s.albedo_id != null) neededAlbedos.add(s.albedo_id);
        if (s.normal_id != null) neededAlbedos.add(s.normal_id);
        if (s.emissive_id != null) neededAlbedos.add(s.emissive_id);

        const mesh = new THREE.Mesh(geom, material);
        mesh.name = `${inst.name || inst.tuid}_sm${i}`;
        node.add(mesh);
      }
    }
    root.add(node);

    
    if (idx % 32 === 0) {
      const frac = 0.05 + (idx / selectedInstances.length) * 0.25;
      emit({
        phase: "preparing",
        label: `Building scene (${idx + 1}/${selectedInstances.length})`,
        fraction: frac,
        detail: path,
      });
      await yieldToBrowser();
    }
  }

  
  emit({
    phase: "decoding-textures",
    label: `Decoding ${neededAlbedos.size} texture(s)`,
    fraction: 0.35,
    detail: path,
  });
  await yieldToBrowser();

  const textureMap = new Map<number, THREE.Texture>();
  let texCount = 0;
  for (const id of neededAlbedos) {
    const blob = textureBlobs?.get(id);
    if (!blob) continue;
    const tex = await loadOneTexture(blob);
    textureMap.set(id, tex);
    texCount++;
    const frac = 0.35 + (texCount / Math.max(1, neededAlbedos.size)) * 0.2;
    emit({
      phase: "decoding-textures",
      label: `Decoded ${texCount}/${neededAlbedos.size} textures`,
      fraction: frac,
      detail: path,
    });
  }

  
  
  
  
  
  
  
  
  const slotRe = /^slots_a([^_]+|_)_n([^_]+|_)_e([^_]+|_)$/;
  root.traverse((obj) => {
    if (!(obj instanceof THREE.Mesh)) return;
    const mat = obj.material as THREE.MeshStandardMaterial;
    if (!mat.name) return;
    const match = slotRe.exec(mat.name);
    if (!match) return;
    const [_, aRaw, nRaw, eRaw] = match;
    const aId = aRaw === "_" ? null : Number(aRaw);
    const nId = nRaw === "_" ? null : Number(nRaw);
    const eId = eRaw === "_" ? null : Number(eRaw);
    const a = aId != null ? textureMap.get(aId) : undefined;
    const n = nId != null ? textureMap.get(nId) : undefined;
    const e = eId != null ? textureMap.get(eId) : undefined;
    if (a) mat.map = a;
    if (n) mat.normalMap = n;
    if (e) mat.emissiveMap = e;
    if (a || n || e) mat.needsUpdate = true;
  });

  
  emit({
    phase: "encoding",
    label: "Encoding binary glTF",
    fraction: 0.6,
    detail: path,
  });
  await yieldToBrowser();

  const exporter = new GLTFExporter();
  
  
  
  
  const origWarn = console.warn;
  console.warn = (...args: unknown[]) => {
    if (
      typeof args[0] === "string" &&
      args[0].includes("Creating normalized normal attribute")
    ) {
      return;
    }
    origWarn.apply(console, args);
  };
  let bytes: ArrayBuffer;
  try {
    bytes = await new Promise<ArrayBuffer>((resolve, reject) => {
      exporter.parse(
        root,
        (gltf) => {
          if (gltf instanceof ArrayBuffer) resolve(gltf);
          else
            reject(new Error("GLTFExporter returned JSON; expected binary"));
        },
        (err) => reject(err),
        {
          binary: true,
          includeCustomExtensions: false,
          embedImages: true,
          
          
          
          
          animations: animationClips,
        },
      );
    });
  } finally {
    console.warn = origWarn;
    
    
    
    for (const rig of skinnedRigsToDispose) rig.dispose();
  }

  
  emit({
    phase: "writing",
    label: `Writing ${formatBytes(bytes.byteLength)} to disk`,
    fraction: 0.9,
    detail: path,
  });
  await yieldToBrowser();

  await invoke<void>("write_bytes", {
    path,
    bytes: Array.from(new Uint8Array(bytes)),
  });

  emit({
    phase: "done",
    label: `Saved ${formatBytes(bytes.byteLength)}`,
    fraction: 1,
    detail: path,
  });

  return { path, bytes: bytes.byteLength };
}


function yieldToBrowser(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => resolve());
    } else {
      setTimeout(resolve, 0);
    }
  });
}

function loadOneTexture(blob: Blob): Promise<THREE.Texture> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(blob);
    const img = new Image();
    img.onload = () => {
      const tex = new THREE.Texture(img);
      tex.colorSpace = THREE.SRGBColorSpace;
      tex.flipY = false;
      tex.needsUpdate = true;
      URL.revokeObjectURL(url);
      resolve(tex);
    };
    img.onerror = (e) => {
      URL.revokeObjectURL(url);
      reject(e);
    };
    img.src = url;
  });
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
