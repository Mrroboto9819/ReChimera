import { useEffect, useMemo } from "react";
import { Canvas } from "@react-three/fiber";
import { Bounds, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import {
  type AssetMeshes,
  type Instance,
  type LevelMeshes,
  type TextureBlobMap,
} from "../api";
import { buildSkinnedAsset, type BuiltSkinnedAsset } from "../skinning";
import { GlbPreview, type ExportPicks } from "./GlbPreview";

interface AssetPreviewProps {
  instance: Instance | null;
  meshes: LevelMeshes | null;
  textureBlobs: TextureBlobMap | null;
  cacheFolder?: string;
  showOverlay?: boolean;
  exportPicks?: ExportPicks;
  onExportPicksChange?: (picks: ExportPicks) => void;
}

export function AssetPreview({
  instance,
  meshes,
  textureBlobs,
  cacheFolder,
  showOverlay,
  exportPicks,
  onExportPicksChange,
}: AssetPreviewProps) {
  if (!instance) {
    return (
      <div className="asset-preview asset-preview-empty">
        <span className="dim small">No selection</span>
      </div>
    );
  }

  if (cacheFolder && (instance.kind === "moby" || instance.kind === "tie")) {
    return (
      <div className="asset-preview">
        <GlbPreview
          folder={cacheFolder}
          assetTuidHex={instance.asset_tuid.split("#")[0]!}
          kind={instance.kind}
          assetName={instance.name}
          showOverlay={showOverlay}
          exportPicks={exportPicks}
          onExportPicksChange={onExportPicksChange}
        />
        <span className="asset-preview-tag mono small">
          {instance.tuid.split("#")[0]}
        </span>
      </div>
    );
  }

  if (!meshes) {
    return (
      <div className="asset-preview">
        <ProxyPreview kind={instance.kind} />
        <span className="asset-preview-tag mono small">proxy</span>
      </div>
    );
  }

  return (
    <div className="asset-preview">
      <PreviewScene
        instance={instance}
        meshes={meshes}
        textureBlobs={textureBlobs}
      />
      <span className="asset-preview-tag mono small">
        {instance.tuid.split("#")[0]}
      </span>
    </div>
  );
}

function ProxyPreview({ kind }: { kind: Instance["kind"] }) {
  const color = kind === "moby" ? "#55b3ff" : "#5fc992";
  const scale: [number, number, number] =
    kind === "moby" ? [1.4, 1.4, 1.4] : [2.2, 1.2, 2.2];
  return (
    <Canvas camera={{ position: [3, 2.4, 3], fov: 42, near: 0.01, far: 100 }}>
      <color attach="background" args={["#0b0c0e"]} />
      <ambientLight intensity={0.8} />
      <directionalLight position={[5, 8, 5]} intensity={0.9} />
      <gridHelper args={[5, 10, "#20242a", "#15181d"]} position={[0, -0.75, 0]} />
      <group scale={scale}>
        <mesh>
          <boxGeometry args={[1, 1, 1]} />
          <meshBasicMaterial color={color} wireframe transparent opacity={0.72} />
        </mesh>
      </group>
      <OrbitControls
        makeDefault
        enableDamping
        dampingFactor={0.1}
        autoRotate
        autoRotateSpeed={0.8}
      />
    </Canvas>
  );
}

function PreviewScene({
  instance,
  meshes,
  textureBlobs,
}: {
  instance: Instance;
  meshes: LevelMeshes;
  textureBlobs: TextureBlobMap | null;
}) {
  const asset = useMemo<AssetMeshes | undefined>(() => {
    return (
      meshes.moby_assets.find((a) => a.asset_tuid === instance.asset_tuid) ??
      meshes.tie_assets.find((a) => a.asset_tuid === instance.asset_tuid)
    );
  }, [instance.asset_tuid, meshes.moby_assets, meshes.tie_assets]);

  const built = useMemo<BuiltSkinnedAsset | null>(() => {
    if (!asset) return null;
    return buildSkinnedAsset(asset);
  }, [asset]);

  useEffect(() => {
    return () => {
      built?.dispose();
    };
  }, [built]);

  const textureMap = useMemo(() => {
    if (!asset || !textureBlobs) return new Map<number, THREE.Texture>();
    const ids = new Set<number>();
    for (const sm of asset.submeshes) {
      if (sm.albedo_id != null) ids.add(sm.albedo_id);
      if (sm.normal_id != null) ids.add(sm.normal_id);
      if (sm.emissive_id != null) ids.add(sm.emissive_id);
    }
    const m = new Map<number, THREE.Texture>();
    for (const id of ids) {
      const blob = textureBlobs.get(id);
      if (!blob) continue;
      const url = URL.createObjectURL(blob);
      const img = new Image();
      const tex = new THREE.Texture(img);
      tex.colorSpace = THREE.SRGBColorSpace;
      tex.flipY = false;
      tex.wrapS = THREE.RepeatWrapping;
      tex.wrapT = THREE.RepeatWrapping;
      tex.minFilter = THREE.LinearFilter;
      tex.magFilter = THREE.LinearFilter;
      tex.generateMipmaps = false;
      img.onload = () => {
        tex.needsUpdate = true;
        URL.revokeObjectURL(url);
      };
      img.src = url;
      m.set(id, tex);
    }
    return m;
  }, [asset, textureBlobs]);

  useEffect(() => {
    return () => {
      for (const tex of textureMap.values()) tex.dispose();
    };
  }, [textureMap]);

  if (built && asset) {
    for (let i = 0; i < built.materials.length; i++) {
      const mat = built.materials[i] as THREE.MeshStandardMaterial;
      const sub = asset.submeshes[i];
      if (!sub) continue;
      const albedo =
        sub.albedo_id != null ? textureMap.get(sub.albedo_id) ?? null : null;
      const normal =
        sub.normal_id != null ? textureMap.get(sub.normal_id) ?? null : null;
      const emissive =
        sub.emissive_id != null
          ? textureMap.get(sub.emissive_id) ?? null
          : null;
      let touched = false;
      if (albedo && mat.map !== albedo) {
        mat.map = albedo;
        touched = true;
      }
      if (normal && mat.normalMap !== normal) {
        mat.normalMap = normal;
        touched = true;
      }
      if (emissive && mat.emissiveMap !== emissive) {
        mat.emissiveMap = emissive;
        mat.emissive = new THREE.Color(0xffffff);
        mat.emissiveIntensity = 0.7;
        touched = true;
      }
      if (touched) mat.needsUpdate = true;
    }
  }

  if (!asset) {
    return (
      <div className="asset-preview-empty">
        <span className="dim small">Geometry not decoded yet</span>
      </div>
    );
  }
  if (!built || built.skinnedMeshes.length === 0) {
    return (
      <div className="asset-preview-empty">
        <span className="dim small">Asset has no submeshes</span>
      </div>
    );
  }

  return (
    <Canvas
      camera={{ position: [3, 2, 3], fov: 40, near: 0.01, far: 10000 }}
      dpr={[1, 1.5]}
    >
      <color attach="background" args={["#0b0c0e"]} />
      <ambientLight intensity={0.55} />
      <directionalLight position={[5, 10, 7.5]} intensity={1.1} />
      <directionalLight position={[-5, -2, -5]} intensity={0.35} />

      <Bounds fit clip observe margin={1.2}>
        <group>
          <primitive object={built.root} />
          {built.bones.length > 0 && (
            <primitive object={new THREE.SkeletonHelper(built.root)} />
          )}
        </group>
      </Bounds>

      <OrbitControls
        makeDefault
        enableDamping
        dampingFactor={0.1}
        autoRotate
        autoRotateSpeed={0.6}
      />
    </Canvas>
  );
}
