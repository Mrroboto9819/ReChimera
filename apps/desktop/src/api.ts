import { Channel, invoke } from "@tauri-apps/api/core";

export interface Section {
  id: number;
  offset: number;
  count: number;
  length: number;
}

export interface AssetCount {
  kind: AssetKind;
  section_id: number;
  count: number;
  present: boolean;
}

export interface LevelSummary {
  folder: string;
  version_major: number;
  version_minor: number;
  sections: Section[];
  asset_counts: AssetCount[];
}

export interface AssetPointer {
  
  tuid: string;
  offset: number;
  length: number;
}

export type AssetKind =
  | "shader"
  | "texture"
  | "highmip"
  | "cubemap"
  | "tie"
  | "foliage"
  | "shrub"
  | "moby"
  | "animset"
  | "cinematic"
  | "zone"
  | "lighting"
  | "ufrag"
  | "detail"
  | "light"
  | "envsampler"
  | "sky";

export interface Instance {
  
  tuid: string;
  
  asset_tuid: string;
  kind: AssetKind;
  name: string;
  
  position: [number, number, number];
  
  quaternion: [number, number, number, number];
  
  scale: [number, number, number];
}

export const openLevel = (folder: string) =>
  invoke<LevelSummary>("open_level", { folder });

export const listAssets = (folder: string, kind: AssetKind) =>
  invoke<AssetPointer[]>("list_assets", { folder, kind });

export interface ManifestEntry {
  
  tuid: string;
  offset: number;
  length: number;
}

export interface ManifestGroup {
  kind: AssetKind;
  section_id: number;
  


  decoded: boolean;
  count: number;
  entries: ManifestEntry[];
}

export interface LevelManifest {
  folder: string;
  

  engine: "new" | "old";
  version_major: number;
  version_minor: number;
  sections: Section[];
  groups: ManifestGroup[];
}

export const buildLevelManifest = (folder: string) =>
  invoke<LevelManifest>("build_level_manifest", { folder });



export interface CacheManifestEntry {
  kind: "moby" | "tie" | "detail" | "shrub" | "foliage" | "texture" | "ufrag" | "sky" | "cubemap";
  tuid: string;
  name: string;
  file: string;
  size_bytes: number;
}

export interface CacheManifest {
  version: number;
  folder: string;
  entries: CacheManifestEntry[];
}

export interface CacheStatus {
  exists: boolean;
  folder: string;
  
  cache_path: string;
  entry_count: number;
  mobys: number;
  ties: number;
  textures: number;
  



  stale: boolean;
  




  incomplete: boolean;
}

export type CacheEvent =
  | { type: "phase"; phase: "mobys" | "ties" | "materials" | "normalmaps" | "textures" | "ufrags"; total: number }
  | { type: "item"; kind: "moby" | "tie" | "ufrag" | "texture" | "cubemap"; name: string; file: string }
  | { type: "progress"; current: number }
  | { type: "done"; entry_count: number }
  | { type: "error"; message: string };

export interface CubemapDescriptor {
  tuid: string;
  width: number;
  height: number;
  faces: string[];
}

export const cacheStatus = (folder: string) =>
  invoke<CacheStatus>("cache_status", { folder });

export const readCachedManifest = (folder: string) =>
  invoke<CacheManifest>("read_cached_manifest", { folder });




export const readCachedAsset = (folder: string, file: string) =>
  invoke<unknown>("read_cached_asset", { folder, file });



export const readCachedBytes = (folder: string, file: string) =>
  invoke<ArrayBuffer>("read_cached_bytes", { folder, file });

export const extractLevelToCache = (
  folder: string,
  onEvent: Channel<CacheEvent>,
) => invoke<void>("extract_level_to_cache", { folder, onEvent });

export const reextractLevelCache = (
  folder: string,
  onEvent: Channel<CacheEvent>,
) => invoke<void>("reextract_level_cache", { folder, onEvent });





export const exportCachedMobyGlb = (
  levelFolder: string,
  assetTuidHex: string,
  outPath: string,
) =>
  invoke<number>("export_cached_moby_glb", {
    levelFolder,
    assetTuidHex,
    outPath,
  });

export interface AnimsetClipMeta {
  name: string;
  num_frames: number;
  frame_rate: number;
  looping: boolean;
}

export interface AnimsetSummary {
  hash: string;
  clips: AnimsetClipMeta[];
}

export const listAnimsets = (folder: string) =>
  invoke<AnimsetSummary[]>("list_animsets", { folder });

export interface DecodedBoneDto {
  rotations: number[];
  translations: number[];
  scales: number[];
  rotation_animated: boolean;
  translation_animated: boolean;
  scale_animated: boolean;
}

export interface DecodedClipDto {
  name: string;
  num_frames: number;
  frame_rate: number;
  looping: boolean;
  bones: DecodedBoneDto[];
}

export const decodeAnimsetClip = (
  folder: string,
  assetTuidHex: string,
  animsetHash: string,
  clipIndex: number,
) =>
  invoke<DecodedClipDto>("decode_animset_clip", {
    folder,
    assetTuidHex,
    animsetHash,
    clipIndex,
  });

export interface ClipPick {
  animset_hash: string;
  clip_indices: number[];
}

export interface GlbExportOptions {
  include_mesh: boolean;
  include_materials: boolean;
  include_armature: boolean;
  extra_clips: ClipPick[];
  texture_max_dim?: number | null;
}

export const exportMobyGlbWithOptions = (
  levelFolder: string,
  assetTuidHex: string,
  outPath: string,
  options: GlbExportOptions,
) =>
  invoke<number>("export_moby_glb_with_options", {
    levelFolder,
    assetTuidHex,
    outPath,
    options,
  });

export const writeBytes = (path: string, bytes: number[]) =>
  invoke<void>("write_bytes", { path, bytes });

export type SkyboxFormat = "glb" | "obj" | "ply" | "json";

export interface SkyboxMeta {
  vertex_count: number;
  triangle_count: number;
  aabb_min: [number, number, number];
  aabb_max: [number, number, number];
  texture_offset: number | null;
}

export const exportSkybox = (
  levelFolder: string,
  format: SkyboxFormat,
  outPath: string,
) =>
  invoke<number>("export_skybox", { levelFolder, format, outPath });

export const readCachedSkyboxMeta = (levelFolder: string) =>
  invoke<SkyboxMeta>("read_cached_skybox_meta", { levelFolder });

export const exportTexturePng = (
  levelFolder: string,
  texId: number,
  outPath: string,
) =>
  invoke<number>("export_texture_png", {
    levelFolder,
    texId,
    outPath,
  });

export const exportTextureDds = (
  levelFolder: string,
  texId: number,
  outPath: string,
) =>
  invoke<number>("export_texture_dds", {
    levelFolder,
    texId,
    outPath,
  });

export type LevelGlbExportEvent =
  | { type: "phase"; label: string; total: number }
  | { type: "progress"; current: number }
  | {
      type: "done";
      bytes_written: number;
      instance_count: number;
      asset_count: number;
    }
  | { type: "error"; message: string };

export const exportLevelGlb = (
  levelFolder: string,
  outPath: string,
  onEvent: Channel<LevelGlbExportEvent>,
) =>
  invoke<void>("export_level_glb", {
    levelFolder,
    outPath,
    onEvent,
  });




export async function loadCachedTextures(
  folder: string,
  ids: number[],
  options?: {
    concurrency?: number;
    onBatch?: (out: TextureBlobMap) => void;
  },
): Promise<TextureBlobMap> {
  const out: TextureBlobMap = new Map();
  const concurrency = Math.max(1, options?.concurrency ?? 32);
  const onBatch = options?.onBatch;
  let nextIdx = 0;
  let dirty = false;
  let flushScheduled = false;
  const scheduleFlush = () => {
    if (!onBatch || flushScheduled || !dirty) return;
    flushScheduled = true;
    dirty = false;
    queueMicrotask(() => {
      flushScheduled = false;
      onBatch(out);
      if (dirty) scheduleFlush();
    });
  };
  const worker = async () => {
    while (true) {
      const i = nextIdx++;
      if (i >= ids.length) return;
      const id = ids[i]!;
      try {
        const buf = await readCachedBytes(folder, `textures/${id}.png`);
        out.set(id, new Blob([buf], { type: "image/png" }));
        dirty = true;
        scheduleFlush();
      } catch {
        /* ignore — missing texture */
      }
    }
  };
  const workers: Promise<void>[] = [];
  for (let k = 0; k < Math.min(concurrency, ids.length); k++) {
    workers.push(worker());
  }
  await Promise.all(workers);
  return out;
}

export interface CacheLoadProgress {
  phase: "manifest" | "mobys" | "ties" | "ufrags" | "textures";
  current: number;
  total: number;
}

export async function loadFromCache(
  folder: string,
  onProgress?: (p: CacheLoadProgress) => void,
): Promise<LevelMeshes> {
  onProgress?.({ phase: "manifest", current: 0, total: 1 });
  const manifest = await readCachedManifest(folder);

  const mobyEntries = manifest.entries.filter(
    (e) => e.kind === "moby" && e.file.endsWith(".json"),
  );
  const tieEntries = manifest.entries.filter(
    (e) => e.kind === "tie" && e.file.endsWith(".json"),
  );
  const detailEntries = manifest.entries.filter(
    (e) => e.kind === "detail" && e.file.endsWith(".json"),
  );
  const shrubEntries = manifest.entries.filter(
    (e) => e.kind === "shrub" && e.file.endsWith(".json"),
  );
  const foliageEntries = manifest.entries.filter(
    (e) => e.kind === "foliage" && e.file.endsWith(".json"),
  );
  const ufragEntries = manifest.entries.filter((e) => e.kind === "ufrag");
  const textureEntries = manifest.entries.filter((e) => e.kind === "texture");

  const moby_assets: AssetMeshes[] = [];
  onProgress?.({ phase: "mobys", current: 0, total: mobyEntries.length });
  for (let i = 0; i < mobyEntries.length; i++) {
    try {
      const data = (await readCachedAsset(folder, mobyEntries[i]!.file)) as AssetMeshes;
      moby_assets.push(data);
    } catch {
      /* skip corrupted entry */
    }
    if ((i + 1) % 8 === 0 || i === mobyEntries.length - 1) {
      onProgress?.({ phase: "mobys", current: i + 1, total: mobyEntries.length });
    }
  }

  const tie_assets: AssetMeshes[] = [];
  onProgress?.({ phase: "ties", current: 0, total: tieEntries.length });
  for (let i = 0; i < tieEntries.length; i++) {
    try {
      const data = (await readCachedAsset(folder, tieEntries[i]!.file)) as AssetMeshes;
      tie_assets.push(data);
    } catch {
      /* skip */
    }
    if ((i + 1) % 8 === 0 || i === tieEntries.length - 1) {
      onProgress?.({ phase: "ties", current: i + 1, total: tieEntries.length });
    }
  }

  const detail_assets: AssetMeshes[] = [];
  for (let i = 0; i < detailEntries.length; i++) {
    try {
      const data = (await readCachedAsset(folder, detailEntries[i]!.file)) as AssetMeshes;
      detail_assets.push(data);
    } catch {
      /* skip */
    }
  }

  const shrub_assets: AssetMeshes[] = [];
  for (let i = 0; i < shrubEntries.length; i++) {
    try {
      const data = (await readCachedAsset(folder, shrubEntries[i]!.file)) as AssetMeshes;
      shrub_assets.push(data);
    } catch {
      /* skip */
    }
  }

  const foliage_assets: AssetMeshes[] = [];
  for (let i = 0; i < foliageEntries.length; i++) {
    try {
      const data = (await readCachedAsset(folder, foliageEntries[i]!.file)) as AssetMeshes;
      foliage_assets.push(data);
    } catch {
      /* skip */
    }
  }

  const ufrag_meshes: UFragMesh[] = [];
  onProgress?.({ phase: "ufrags", current: 0, total: ufragEntries.length });
  for (let i = 0; i < ufragEntries.length; i++) {
    try {
      const data = (await readCachedAsset(folder, ufragEntries[i]!.file)) as UFragMesh;
      ufrag_meshes.push(data);
    } catch {
      /* skip */
    }
    if ((i + 1) % 16 === 0 || i === ufragEntries.length - 1) {
      onProgress?.({ phase: "ufrags", current: i + 1, total: ufragEntries.length });
    }
  }

  const textures: TexturePayload[] = textureEntries.map((e) => ({
    id: parseInt(e.tuid, 10),
    width: 0,
    height: 0,
  }));
  onProgress?.({ phase: "textures", current: textures.length, total: textures.length });

  return { moby_assets, tie_assets, detail_assets, shrub_assets, foliage_assets, ufrag_meshes, textures };
}

export interface UFragBounds {
  tuid: string;
  zone_tuid: string;
  position: [number, number, number];
  radius: number;
  vertex_count: number;
  triangle_count: number;
}

export interface LevelLayout {
  instances: Instance[];
  ufrags: UFragBounds[];
}

export const levelLayout = (folder: string) =>
  invoke<LevelLayout>("level_layout", { folder });

export interface MeshGeom {
  positions_b64: string;
  uvs_b64: string;
  indices_b64: string;
  
  albedo_id: number | null;
  
  normal_id: number | null;
  

  emissive_id: number | null;
  

  bone_indices_b64: string;
  
  bone_weights_b64: string;
}

export interface DecodedMeshGeom {
  positions: Float32Array;
  uvs: Float32Array;
  indices: Uint32Array;
  bone_indices: Uint16Array;
  bone_weights: Uint8Array<ArrayBuffer>;
}

export interface SkeletonInfo {
  
  bone_count: number;
  
  root_bone: number;
  
  parents: number[];
  



  bind_local: number[][];
  
  bind_world_inverse: number[][];
  


  tms0_col?: number[][];
  
  tms1_col?: number[][];
  
  scale_shift: number;
  


  translation_shift: number;
}

export interface AssetMeshes {
  asset_tuid: string;
  


  name: string;
  submeshes: MeshGeom[];
  


  skeleton: SkeletonInfo | null;
  


  animset_hash: string | null;



  bind_pose_inverse_offset: number;

  embedded_animation_count?: number;
}

export interface UFragMesh {
  tuid: string;
  zone_tuid: string;
  
  position: [number, number, number];
  mesh: MeshGeom;
}







export interface TexturePayload {
  id: number;
  width: number;
  height: number;
}




export type TextureBlobMap = Map<number, Blob>;

export interface LevelMeshes {
  moby_assets: AssetMeshes[];
  tie_assets: AssetMeshes[];
  detail_assets?: AssetMeshes[];
  shrub_assets?: AssetMeshes[];
  foliage_assets?: AssetMeshes[];
  ufrag_meshes: UFragMesh[];
  textures: TexturePayload[];
}







export type PhaseId =
  | "layout"
  | "shaders"
  | "mobys"
  | "ties"
  | "ufrags"
  | "textures";

export type LevelEvent =
  | {
      type: "phase";
      phase: PhaseId;
      label: string;
      total: number;
      

      chunk_size: number;
    }
  | { type: "progress"; current: number }
  | { type: "moby_asset"; asset: AssetMeshes }
  | { type: "tie_asset"; asset: AssetMeshes }
  | { type: "ufrag_mesh"; mesh: UFragMesh }
  | { type: "texture"; texture: TexturePayload }
  | { type: "done" }
  | { type: "error"; message: string };



export function streamLevelMeshes(
  folder: string,
  onEvent: (e: LevelEvent) => void,
): Promise<void> {
  const ch = new Channel<LevelEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("level_meshes_stream", { folder, onEvent: ch });
}






export type CharacterLibraryEvent =
  | { type: "missing" }
  | { type: "located"; path: string }
  | { type: "total"; total: number }
  | { type: "asset"; asset: AssetMeshes }
  | { type: "texture"; texture: TexturePayload }
  | { type: "done" }
  | { type: "error"; message: string };

export function streamCharacterLibrary(
  folder: string,
  onEvent: (e: CharacterLibraryEvent) => void,
): Promise<void> {
  const ch = new Channel<CharacterLibraryEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("level_character_library_stream", {
    folder,
    onEvent: ch,
  });
}

export function decodeMeshGeom(mesh: MeshGeom): DecodedMeshGeom {
  return {
    positions: decodeFloat32(mesh.positions_b64),
    uvs: decodeFloat32(mesh.uvs_b64),
    indices: decodeUint32(mesh.indices_b64),
    bone_indices: decodeUint16(mesh.bone_indices_b64),
    bone_weights: decodeUint8(mesh.bone_weights_b64),
  };
}

function decodeBytes(b64: string): Uint8Array<ArrayBuffer> {
  if (b64.length === 0) return new Uint8Array(new ArrayBuffer(0));
  const bin = atob(b64);
  const bytes = new Uint8Array(new ArrayBuffer(bin.length));
  for (let i = 0; i < bin.length; i++) {
    bytes[i] = bin.charCodeAt(i);
  }
  return bytes;
}

function decodeFloat32(b64: string): Float32Array {
  const bytes = decodeBytes(b64);
  return new Float32Array(bytes.buffer);
}

function decodeUint32(b64: string): Uint32Array {
  const bytes = decodeBytes(b64);
  return new Uint32Array(bytes.buffer);
}

function decodeUint16(b64: string): Uint16Array {
  const bytes = decodeBytes(b64);
  return new Uint16Array(bytes.buffer);
}

function decodeUint8(b64: string): Uint8Array<ArrayBuffer> {
  return decodeBytes(b64);
}







export interface GltfFile {
  
  name: string;
  
  path: string;
  
  extension: string;
  size_bytes: number;
  

  category: string;
}

export interface GltfLibrary {
  
  folder: string;
  files: GltfFile[];
}




export const listCharacterGltfs = (folder: string) =>
  invoke<GltfLibrary>("list_character_gltfs", { folder });





export const listEntitiesGltfs = (folder: string) =>
  invoke<GltfLibrary>("list_entities_gltfs", { folder });




export const listGltfsInFolder = (path: string) =>
  invoke<GltfLibrary>("list_gltfs_in_folder", { path });








export const readFileBytes = (path: string) =>
  invoke<ArrayBuffer>("read_file_bytes", { path });





export interface DecodedBone {
  

  rotations: number[];
  

  translations: number[];
  scales: number[];
  rotation_animated: boolean;
  translation_animated: boolean;
  scale_animated: boolean;
}

export interface DecodedClip {
  name: string;
  num_frames: number;
  frame_rate: number;
  looping: boolean;
  


  bones: DecodedBone[];
}




export const fetchAnimsetClip = (
  level_folder: string,
  animset_hash_hex: string,
  bind_pose_inverse_offset: number,
  scale_shift: number,
) => {
  const safePosOffset = Number.isFinite(bind_pose_inverse_offset)
    ? bind_pose_inverse_offset
    : 0;
  const safeScaleShift = Number.isFinite(scale_shift) ? scale_shift : 0;
  const positionScale = Math.pow(2, safePosOffset);
  const scaleScale = Math.pow(2, safeScaleShift);
  return invoke<DecodedClip>("fetch_animset_clip", {
    levelFolder: level_folder,
    animsetHashHex: animset_hash_hex,
    positionScale: Number.isFinite(positionScale) ? positionScale : 1,
    scaleScale: Number.isFinite(scaleScale) ? scaleScale : 1,
  });
};

export interface AnimsetSummary {
  tuid_hex: string;
  name: string;
  num_frames: number;
  frame_rate: number;
  num_bones: number;
  looping: boolean;
}




export const listAnimsetClips = (level_folder: string) =>
  invoke<AnimsetSummary[]>("list_animset_clips", {
    levelFolder: level_folder,
  });

export interface GlbMaterialTextures {
  material_name: string;
  
  albedo_path: string | null;
  
  normal_path: string | null;
  
  emissive_path: string | null;
}





export const findGlbTextures = (
  level_folder: string,
  material_names: string[],
) =>
  invoke<GlbMaterialTextures[]>("find_glb_textures", {
    levelFolder: level_folder,
    materialNames: material_names,
  });





export type SoundCategory = "sfx" | "dialog" | "music";

/// Classify a sound entry by its source filename. Works for all 4 supported
/// games — Insomniac uses consistent naming:
///   - `*dialogue*` / `*voice*` → dialog
///   - `*music*` and various music-related tokens → music
///   - `*sound*` and everything else → sfx
///
/// Music heuristic notes:
///   - `music` is the obvious one.
///   - `mus` is the engine's abbreviation but only when it sits at a word
///     boundary (e.g. `mus_intro`, `level_mus`, `mus.bnk`) so we don't
///     match unrelated words that happen to contain those letters.
///   - `bgm` / `theme` / `ost` are common game-music tags.
const MUSIC_PATTERN = /(^|[^a-z])(music|mus|bgm|theme|ost)([^a-z]|$)/;
export function classifySound(source: string): SoundCategory {
  const s = source.toLowerCase();
  if (s.includes("dialogue") || s.includes("voice")) return "dialog";
  if (MUSIC_PATTERN.test(s)) return "music";
  return "sfx";
}

export interface SoundEntry {
  name: string;

  index: number;


  













  kind: "bank" | "stream" | "raw" | "stream-missing";
  


  source: string;
}

export interface ExtractedSound {
  name: string;
  sample_rate: number;
  

  channels: number;
  sample_count: number;
  
  wav_b64: string;
}



export const listLevelSounds = (level_folder: string) =>
  invoke<SoundEntry[]>("list_level_sounds", { levelFolder: level_folder });


export const extractLevelSounds = (level_folder: string) =>
  invoke<ExtractedSound[]>("extract_level_sounds", {
    levelFolder: level_folder,
  });

export const extractOneSound = (
  level_folder: string,
  name: string,
  source?: string,
) =>
  invoke<ExtractedSound>("extract_one_sound", {
    levelFolder: level_folder,
    name,
    source,
  });

export const extractOneStreamSound = (
  level_folder: string,
  name: string,
  source: string,
) =>
  invoke<ExtractedSound>("extract_one_stream_sound", {
    levelFolder: level_folder,
    name,
    source,
  });





export const extractLevelStreamSounds = (level_folder: string, bank_filename: string) =>
  invoke<ExtractedSound[]>("extract_level_stream_sounds", {
    levelFolder: level_folder,
    bankFilename: bank_filename,
  });




export const extractRawStreamingSounds = (level_folder: string, stream_filename: string) =>
  invoke<ExtractedSound[]>("extract_raw_streaming_sounds", {
    levelFolder: level_folder,
    streamFilename: stream_filename,
  });







export const dumpSoundBank = (level_folder: string, bank_filename: string) =>
  invoke<string>("dump_sound_bank", {
    levelFolder: level_folder,
    bankFilename: bank_filename,
  });





export function wavBlobAndUrl(wav_b64: string): { url: string; bytes: Uint8Array } {
  const bin = atob(wav_b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  const url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
  return { url, bytes };
}


export function wavBlobUrl(wav_b64: string): string {
  return wavBlobAndUrl(wav_b64).url;
}

export interface LevelFile {
  name: string;
  size_bytes: number;
  
  category:
    | "lookup"
    | "core"
    | "audio"
    | "audio-stream"
    | "localization"
    | "lipsync"
    | "lighting"
    | "vfx"
    | "cinematic"
    | "foliage"
    | "config"
    | "other";
  

  parsed: boolean;
}





export const listLevelFiles = (level_folder: string) =>
  invoke<LevelFile[]>("list_level_files", { levelFolder: level_folder });










export async function getLevelTexturePng(
  level_folder: string,
  texture_id: number,
): Promise<Blob> {
  const buf = await invoke<ArrayBuffer>("get_level_texture_png", {
    levelFolder: level_folder,
    textureId: texture_id,
  });
  return new Blob([buf], { type: "image/png" });
}













export async function getLevelTexturesBulk(
  level_folder: string,
  texture_ids: number[],
): Promise<TextureBlobMap> {
  const out: TextureBlobMap = new Map();
  if (texture_ids.length === 0) return out;
  const buf = await invoke<ArrayBuffer>("get_level_textures_bulk", {
    levelFolder: level_folder,
    textureIds: texture_ids,
  });
  const dv = new DataView(buf);
  const count = dv.getUint32(0, true);
  let offset = 4;
  for (let i = 0; i < count; i++) {
    const id = dv.getUint32(offset, true);
    const len = dv.getUint32(offset + 4, true);
    offset += 8;
    
    
    const slice = new Uint8Array(buf, offset, len);
    out.set(id, new Blob([slice], { type: "image/png" }));
    offset += len;
  }
  return out;
}





export interface PsarcEntryDto {
  name: string;
  uncompressed_size: number;
  file_offset: number;
}

export interface PsarcListDto {
  major: number;
  minor: number;
  compression: "zlib" | "lzma" | "oodle" | string;
  block_size: number;
  entry_count: number;
  entries: PsarcEntryDto[];
}

export const psarcList = (path: string) =>
  invoke<PsarcListDto>("psarc_list", { path });

export type PsarcEvent =
  | { type: "total"; total: number }
  | { type: "file"; index: number; name: string; bytes: number }
  | { type: "done" }
  | { type: "error"; message: string };

export function psarcExtractStream(
  input: string,
  output: string,
  onEvent: (e: PsarcEvent) => void,
): Promise<void> {
  const ch = new Channel<PsarcEvent>();
  ch.onmessage = onEvent;
  return invoke<void>("psarc_extract_stream", { input, output, onEvent: ch });
}
