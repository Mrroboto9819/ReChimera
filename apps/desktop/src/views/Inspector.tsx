import { useState } from "react";
import type { Instance, LevelMeshes, TextureBlobMap } from "../api";
import { AssetPreview } from "./AssetPreview";
import type { useEdits } from "../edits";
import { Crosshair, Download, Maximize2, RefreshCw } from "lucide-react";
import { Button, NumberInput } from "../ui";

type Edits = ReturnType<typeof useEdits>;

interface InspectorProps {

  selected: Instance | null;

  selectionCount: number;

  meshes: LevelMeshes | null;



  textureBlobs: TextureBlobMap | null;

  instances: Instance[];


  edits: Edits;

  cacheFolder?: string | null;

  onExportSelected?: () => void;
  onLoadMeshes?: () => void;
  loadingMeshes?: boolean;


  onFocusSelected?: () => void;
  /** Open the currently-selected asset in the AssetWorkbench center tab. */
  onOpenInWorkbench?: (assetTuidHex: string, kind: "moby" | "tie") => void;
}




function quatToEulerDeg(q: [number, number, number, number]): [number, number, number] {
  const [x, y, z, w] = q;
  
  const sinr_cosp = 2 * (w * x + y * z);
  const cosr_cosp = 1 - 2 * (x * x + y * y);
  const roll = Math.atan2(sinr_cosp, cosr_cosp);
  const sinp = 2 * (w * y - z * x);
  const pitch = Math.abs(sinp) >= 1 ? (Math.PI / 2) * Math.sign(sinp) : Math.asin(sinp);
  const siny_cosp = 2 * (w * z + x * y);
  const cosy_cosp = 1 - 2 * (y * y + z * z);
  const yaw = Math.atan2(siny_cosp, cosy_cosp);
  const r2d = 180 / Math.PI;
  return [roll * r2d, pitch * r2d, yaw * r2d];
}

function eulerDegToQuat([rx, ry, rz]: [number, number, number]): [number, number, number, number] {
  const d2r = Math.PI / 180;
  const cx = Math.cos((rx * d2r) / 2);
  const sx = Math.sin((rx * d2r) / 2);
  const cy = Math.cos((ry * d2r) / 2);
  const sy = Math.sin((ry * d2r) / 2);
  const cz = Math.cos((rz * d2r) / 2);
  const sz = Math.sin((rz * d2r) / 2);
  return [
    sx * cy * cz - cx * sy * sz,
    cx * sy * cz + sx * cy * sz,
    cx * cy * sz - sx * sy * cz,
    cx * cy * cz + sx * sy * sz,
  ];
}







export function Inspector({
  selected,
  selectionCount,
  meshes,
  textureBlobs,
  edits,
  cacheFolder,
  onExportSelected,
  onLoadMeshes,
  loadingMeshes = false,
  onFocusSelected,
  onOpenInWorkbench,
}: InspectorProps) {
  const [exporting, setExporting] = useState(false);

  const canExport = selectionCount > 0 && meshes != null;
  const canFocus = selected != null;
  const needsMeshes = selected != null && meshes == null;
  const isModified = selected ? edits.isModified(selected.tuid) : false;
  const selectedName = selected?.name || selected?.tuid.split("#")[0] || "";

  // Live (with-edits-applied) view of the selected instance, so the
  // input fields reflect the in-progress drag from the viewport gizmo.
  const live = (() => {
    if (!selected) return null;
    const e = edits.edits.get(selected.tuid);
    return e
      ? { ...selected, position: e.position, quaternion: e.quaternion, scale: e.scale }
      : selected;
  })();
  const liveEuler = live ? quatToEulerDeg(live.quaternion) : null;

  const setField = (
    axis: "x" | "y" | "z",
    kind: "position" | "rotation" | "scale",
    value: number,
  ) => {
    if (!selected || !live) return;
    const idx = axis === "x" ? 0 : axis === "y" ? 1 : 2;
    if (kind === "position") {
      const next = [...live.position] as [number, number, number];
      next[idx] = value;
      edits.setEdit(selected.tuid, { position: next }, selected);
    } else if (kind === "scale") {
      const next = [...live.scale] as [number, number, number];
      next[idx] = value;
      edits.setEdit(selected.tuid, { scale: next }, selected);
    } else {
      const cur = liveEuler ?? [0, 0, 0];
      const next: [number, number, number] = [cur[0]!, cur[1]!, cur[2]!];
      next[idx] = value;
      edits.setEdit(
        selected.tuid,
        { quaternion: eulerDegToQuat(next) },
        selected,
      );
    }
  };

  const handleExport = async () => {
    if (!onExportSelected || exporting) return;
    setExporting(true);
    try {
      await onExportSelected();
    } finally {
      setExporting(false);
    }
  };

  
  
  
  const hierarchyPath = selected
    ? [
        "Workspace",
        capitalize(selected.kind) + "s",
        selected.name || selected.tuid.split("#")[0],
      ].join(" › ")
    : null;

  return (
    <div className="panel pane-inspector view-flush">
      <div className="panel-body">
        {selectionCount > 1 && (
          <div className="multi-select-banner">
            <strong>{selectionCount.toLocaleString()}</strong> objects selected
            <span className="dim small">
              · showing details for the most recent
            </span>
          </div>
        )}

        {selected && (
          <div className="inspector-hero">
            <div className="inspector-hero-main">
              <span className={`tree-icon kind-${selected.kind}`}>
                {selected.kind[0]?.toUpperCase()}
              </span>
              <div className="inspector-hero-text">
                <strong title={selectedName}>{selectedName}</strong>
                <span className="mono small dim" title={selected.asset_tuid}>
                  {selected.kind} asset {selected.asset_tuid.slice(0, 10)}
                </span>
              </div>
            </div>
            <span className={meshes ? "inspector-data-pill ready" : "inspector-data-pill"}>
              {meshes ? "mesh ready" : "proxy"}
            </span>
          </div>
        )}

        <div className="inspector-preview-wrap">
          <AssetPreview
            instance={selected}
            meshes={meshes}
            textureBlobs={textureBlobs}
            cacheFolder={cacheFolder ?? undefined}
          />
        </div>

        {hierarchyPath && (
          <div className="inspector-path mono small" title={hierarchyPath}>
            {hierarchyPath}
          </div>
        )}

        <div className="inspector-actions">
          <Button
            icon={Crosshair}
            onClick={() => onFocusSelected?.()}
            disabled={!canFocus}
            title={
              canFocus
                ? "Re-frame the viewport on this object"
                : "Select an object to navigate to it"
            }
          >
            Go to
          </Button>
          {(selected?.kind === "moby" || selected?.kind === "tie") && (
            <Button
              icon={Maximize2}
              onClick={() => {
                if (!selected || !onOpenInWorkbench) return;
                const tuid = selected.asset_tuid.split("#")[0];
                if (!tuid) return;
                onOpenInWorkbench(
                  tuid,
                  selected.kind as "moby" | "tie",
                );
              }}
              disabled={!onOpenInWorkbench}
              title="Open this asset in the Asset workbench center tab"
            >
              View model
            </Button>
          )}
          {needsMeshes && (
            <Button
              variant="primary"
              icon={RefreshCw}
              onClick={() => onLoadMeshes?.()}
              disabled={!onLoadMeshes}
              loading={loadingMeshes}
              title={
                loadingMeshes
                  ? "Mesh decode is running in the background — interact freely"
                  : "Retry mesh decode if the auto-load failed"
              }
            >
              {loadingMeshes ? "Loading meshes…" : "Reload meshes"}
            </Button>
          )}
          <Button
            variant="primary"
            icon={Download}
            onClick={handleExport}
            disabled={!canExport}
            loading={exporting}
            title={
              canExport
                ? `Export ${selectionCount} object(s) as .glb`
                : "Select at least one object to export"
            }
          >
            {exporting
              ? "Exporting…"
              : selectionCount > 0
                ? `Export ${selectionCount} as .glb`
                : "Export .glb"}
          </Button>
        </div>

        {selected ? (
          <div className="inspector-content">
            {!meshes && (
              <div className="inspector-section inspector-section-muted">
                <h4>Preview mode</h4>
                <p className="dim small">
                  Showing the proxy object. Load meshes when you need the real
                  model, textures, or GLB export.
                </p>
              </div>
            )}
            <div className="inspector-section">
              <h4>Identity</h4>
              <dl className="kv">
                <dt>Name</dt>
                <dd>{selected.name || <span className="dim">unnamed</span>}</dd>
                <dt>Kind</dt>
                <dd>{selected.kind}</dd>
                <dt>Instance</dt>
                <dd className="mono small">{selected.tuid.split("#")[0]}</dd>
                <dt>Asset</dt>
                <dd className="mono small">{selected.asset_tuid}</dd>
              </dl>
            </div>
            {(() => {
              
              
              if (!meshes) return null;
              const asset =
                meshes.moby_assets.find((a) => a.asset_tuid === selected.asset_tuid) ??
                meshes.tie_assets.find((a) => a.asset_tuid === selected.asset_tuid);
              if (!asset) return null;

              const rig = asset.skeleton;

              const albedos = new Set<number>();
              const normals = new Set<number>();
              const emissives = new Set<number>();
              for (const sm of asset.submeshes) {
                if (sm.albedo_id != null) albedos.add(sm.albedo_id);
                if (sm.normal_id != null) normals.add(sm.normal_id);
                if (sm.emissive_id != null) emissives.add(sm.emissive_id);
              }
              const totalTex = albedos.size + normals.size + emissives.size;

              if (!rig && totalTex === 0) return null;

              return (
                <>
                  {rig && (
                    <div className="inspector-section">
                      <h4>Rig</h4>
                      <dl className="kv">
                        <dt>Bones</dt>
                        <dd className="mono small">
                          {rig.bone_count.toLocaleString()}
                        </dd>
                        <dt>Root bone</dt>
                        <dd className="mono small">{rig.root_bone}</dd>
                        <dt>Bind pose</dt>
                        <dd className="mono small">
                          {rig.bind_local.length > 0 ? "local + world⁻¹" : "world⁻¹ only"}
                        </dd>
                      </dl>
                      <p className="dim small" style={{ marginTop: 6 }}>
                        Skin weights + animation playback land in the next
                        sessions — see the README plan.
                      </p>
                    </div>
                  )}
                  {totalTex > 0 && (
                    <div className="inspector-section">
                      <h4>Textures used</h4>
                      <div className="texture-slots">
                        {albedos.size > 0 && (
                          <TextureSlot
                            label="Albedo"
                            ids={Array.from(albedos)}
                            color="hsl(202, 100%, 67%)"
                          />
                        )}
                        {normals.size > 0 && (
                          <TextureSlot
                            label="Normal"
                            ids={Array.from(normals)}
                            color="hsl(151, 59%, 59%)"
                          />
                        )}
                        {emissives.size > 0 && (
                          <TextureSlot
                            label="Emissive"
                            ids={Array.from(emissives)}
                            color="hsl(43, 100%, 60%)"
                          />
                        )}
                      </div>
                    </div>
                  )}
                </>
              );
            })()}

            <div className="inspector-section">
              <div className="inspector-section-header">
                <h4>Transform</h4>
                {isModified && (
                  <button
                    type="button"
                    className="panel-icon-btn"
                    onClick={() => edits.resetEdit(selected.tuid)}
                    title="Discard edits to this instance"
                  >
                    Reset
                  </button>
                )}
              </div>
              {live && liveEuler && (
                <div className="transform-grid">
                  <TransformRow
                    label="Pos"
                    values={live.position}
                    onChange={(axis, value) => setField(axis, "position", value)}
                  />
                  <TransformRow
                    label="Rot°"
                    values={liveEuler}
                    step={1}
                    onChange={(axis, value) => setField(axis, "rotation", value)}
                  />
                  <TransformRow
                    label="Scale"
                    values={live.scale}
                    step={0.1}
                    onChange={(axis, value) => setField(axis, "scale", value)}
                  />
                </div>
              )}
              {isModified && (
                <p className="dim small" style={{ marginTop: 6 }}>
                  Modified — Save to disk is not yet implemented; use the
                  Reset button to discard.
                </p>
              )}
            </div>
          </div>
        ) : (
          <div className="tree-empty">
            <strong>Double-click</strong> an object in the viewport, or
            single-click in the hierarchy, to inspect.
            <p className="dim small" style={{ marginTop: 8 }}>
              Hold <span className="kbd">Ctrl</span> to add to selection,{" "}
              <span className="kbd">Shift</span> for range.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}

function capitalize(s: string): string {
  if (s.length === 0) return s;
  return s[0]!.toUpperCase() + s.slice(1);
}

function TextureSlot({
  label,
  ids,
  color,
}: {
  label: string;
  ids: number[];
  color: string;
}) {
  return (
    <div className="texture-slot">
      <span className="texture-slot-label" style={{ color }}>
        {label}
      </span>
      <div className="texture-slot-ids">
        {ids.map((id) => (
          <code key={id} className="texture-slot-id mono small">
            0x{id.toString(16).toUpperCase().padStart(8, "0")}
          </code>
        ))}
      </div>
    </div>
  );
}

function TransformRow({
  label,
  values,
  step = 0.05,
  onChange,
}: {
  label: string;
  values: [number, number, number];
  step?: number;
  onChange: (axis: "x" | "y" | "z", value: number) => void;
}) {
  return (
    <div className="transform-row">
      <span className="transform-row-label">{label}</span>
      {(["x", "y", "z"] as const).map((axis, i) => (
        <label key={axis} className={`transform-input axis-${axis}`}>
          <span className="transform-input-axis">{axis.toUpperCase()}</span>
          <NumberInput
            value={values[i] ?? 0}
            onValueChange={(v) => onChange(axis, v)}
            step={step}
            precision={3}
            spellCheck={false}
          />
        </label>
      ))}
    </div>
  );
}
