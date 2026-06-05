import * as THREE from "three";
import {
  decodeMeshGeom,
  type AssetMeshes,
  type DecodedClip,
} from "./api";





export type BindStrategy = "it" | "direct" | "relunacy";

const tmpMatA = new THREE.Matrix4();
const tmpMatB = new THREE.Matrix4();
const tmpMatC = new THREE.Matrix4();









export interface BuiltSkinnedAsset {
  


  root: THREE.Group;
  materials: THREE.Material[];
  

  skeleton: THREE.Skeleton;
  bones: THREE.Bone[];
  skinnedMeshes: THREE.SkinnedMesh[];
  
  dispose: () => void;
}













export function buildSkinnedAsset(
  asset: AssetMeshes,
  bindStrategy: BindStrategy = "it",
): BuiltSkinnedAsset | null {
  const sk = asset.skeleton;
  if (!sk || sk.bone_count === 0) return null;

  const { localBindMatrices, worldInverseMatrices } = resolveBindMatrices(
    sk,
    bindStrategy,
  );

  const bones: THREE.Bone[] = [];
  for (let i = 0; i < sk.bone_count; i++) {
    const bone = new THREE.Bone();
    bone.name = `bone_${i}`;
    const local = localBindMatrices[i];
    if (local && local.length === 16) {
      tmpMatA.fromArray(local);
      tmpMatA.decompose(bone.position, bone.quaternion, bone.scale);
    }
    bones.push(bone);
  }

  const root = new THREE.Group();
  root.name = `skin_${asset.asset_tuid}`;
  for (let i = 0; i < sk.bone_count; i++) {
    const pi = sk.parents[i] ?? -1;
    if (pi < 0 || pi >= bones.length || pi === i) {
      root.add(bones[i]!);
    } else {
      bones[pi]!.add(bones[i]!);
    }
  }

  let boneInverses: THREE.Matrix4[] | undefined;
  if (worldInverseMatrices.length === sk.bone_count) {
    boneInverses = worldInverseMatrices.map((m) =>
      new THREE.Matrix4().fromArray(m),
    );
  }
  const skeleton = new THREE.Skeleton(bones, boneInverses);


  const materials: THREE.Material[] = [];
  const geometries: THREE.BufferGeometry[] = [];
  const skinnedMeshes: THREE.SkinnedMesh[] = [];
  for (let smi = 0; smi < asset.submeshes.length; smi++) {
    const s = asset.submeshes[smi]!;
    const decoded = decodeMeshGeom(s);


    const geom = new THREE.BufferGeometry();
    geom.setAttribute("position", new THREE.BufferAttribute(decoded.positions, 3));
    if (decoded.uvs.length > 0) {
      geom.setAttribute("uv", new THREE.BufferAttribute(decoded.uvs, 2));
    }
    geom.setIndex(new THREE.BufferAttribute(decoded.indices, 1));
    geom.computeVertexNormals();

    const mat = new THREE.MeshStandardMaterial({
      color: 0xffffff,
      roughness: 0.85,
      metalness: 0,
      
      
      name: `slots_a${s.albedo_id ?? "_"}_n${s.normal_id ?? "_"}_e${s.emissive_id ?? "_"}`,
      emissive: 0x000000,
      emissiveIntensity: 0,
    });
    materials.push(mat);
    geometries.push(geom);

    if (
      decoded.bone_indices.length > 0 &&
      decoded.bone_weights.length === decoded.bone_indices.length
    ) {
      const skinIdx = decoded.bone_indices;
      const skinW = new Float32Array(decoded.bone_weights.length);
      for (let i = 0; i < decoded.bone_weights.length; i++) {
        skinW[i] = decoded.bone_weights[i]! / 255;
      }
      geom.setAttribute("skinIndex", new THREE.BufferAttribute(skinIdx, 4));
      geom.setAttribute("skinWeight", new THREE.BufferAttribute(skinW, 4));

      const skinnedMesh = new THREE.SkinnedMesh(geom, mat);
      skinnedMesh.bind(skeleton);
      root.add(skinnedMesh);
      skinnedMeshes.push(skinnedMesh);
    } else {
      const mesh = new THREE.Mesh(geom, mat);
      root.add(mesh);
    }
  }

  return {
    root,
    materials,
    skeleton,
    bones,
    skinnedMeshes,
    dispose: () => {
      for (const g of geometries) g.dispose();
      for (const m of materials) m.dispose();
      skeleton.dispose();
    },
  };
}










export function buildAnimationClipFromDecoded(
  decoded: DecodedClip,
  numBonesInRig: number,
): THREE.AnimationClip {
  const fps = decoded.frame_rate > 0 ? decoded.frame_rate : 30;
  const dt = 1 / fps;
  const tracks: THREE.KeyframeTrack[] = [];

  const animatedTimes: number[] = [];
  for (let i = 0; i < decoded.num_frames; i++) animatedTimes.push(i * dt);
  const staticTimes: number[] = [0];

  const bonesToUse = Math.min(decoded.bones.length, numBonesInRig);
  for (let b = 0; b < bonesToUse; b++) {
    const bone = decoded.bones[b];
    if (!bone) continue;
    if (bone.rotations.length > 0) {
      const t = bone.rotation_animated ? animatedTimes : staticTimes;
      tracks.push(
        new THREE.QuaternionKeyframeTrack(`bone_${b}.quaternion`, t, bone.rotations),
      );
    }
    if (bone.translations.length > 0) {
      const t = bone.translation_animated ? animatedTimes : staticTimes;
      tracks.push(
        new THREE.VectorKeyframeTrack(`bone_${b}.position`, t, bone.translations),
      );
    }
    if (bone.scales.length > 0) {
      const t = bone.scale_animated ? animatedTimes : staticTimes;
      tracks.push(
        new THREE.VectorKeyframeTrack(`bone_${b}.scale`, t, bone.scales),
      );
    }
  }

  const duration = decoded.num_frames > 0 ? (decoded.num_frames - 1) * dt : 0;
  return new THREE.AnimationClip(decoded.name || "clip", duration, tracks);
}



export function isSkinnedAsset(asset: AssetMeshes): boolean {
  if (!asset.skeleton || asset.skeleton.bone_count === 0) return false;
  return asset.submeshes.some((s) => s.bone_indices_b64.length > 0);
}

interface BindMatrices {
  
  localBindMatrices: number[][];
  
  worldInverseMatrices: number[][];
}




function resolveBindMatrices(
  sk: AssetMeshes["skeleton"] & {},
  strategy: BindStrategy,
): BindMatrices {
  if (strategy === "it" || !sk.tms0_col || !sk.tms1_col) {
    return {
      localBindMatrices: sk.bind_local,
      worldInverseMatrices: sk.bind_world_inverse,
    };
  }

  if (strategy === "direct") {
    
    return {
      localBindMatrices: sk.tms0_col,
      worldInverseMatrices: sk.tms1_col,
    };
  }

  
  return {
    localBindMatrices: sk.tms1_col,
    worldInverseMatrices: sk.tms0_col,
  };
}





export function applyBindStrategy(
  rig: BuiltSkinnedAsset,
  asset: AssetMeshes,
  strategy: BindStrategy,
): void {
  const sk = asset.skeleton;
  if (!sk) return;
  const { localBindMatrices, worldInverseMatrices } = resolveBindMatrices(
    sk,
    strategy,
  );
  for (let i = 0; i < rig.bones.length; i++) {
    const local = localBindMatrices[i];
    if (!local || local.length !== 16) continue;
    tmpMatB.fromArray(local);
    tmpMatB.decompose(
      rig.bones[i]!.position,
      rig.bones[i]!.quaternion,
      rig.bones[i]!.scale,
    );
  }
  if (worldInverseMatrices.length === rig.bones.length) {
    for (let i = 0; i < rig.bones.length; i++) {
      tmpMatC.fromArray(worldInverseMatrices[i]!);
      rig.skeleton.boneInverses[i]!.copy(tmpMatC);
    }
  }
  
  
  rig.skeleton.update();
}
