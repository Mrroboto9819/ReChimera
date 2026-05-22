import { useEffect, useMemo, useState } from "react";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { Modal } from "./Modal";
import {
  type AnimsetSummary,
  type ClipPick,
  type GlbExportOptions,
  exportCachedMobyGlb,
  exportMobyGlbWithOptions,
  listAnimsets,
} from "../api";

interface ExportOptionsModalProps {
  open: boolean;
  folder: string;
  assetTuidHex: string;
  assetName: string;
  hasSkeleton: boolean;
  primaryAnimsetHash: string | null;
  initialExtraPicks?: Record<string, number[]>;
  onClose: () => void;
  onExported?: (path: string, bytes: number) => void;
}

type StepId = "scope" | "anims" | "saving";

interface PerAnimsetSelection {
  hash: string;
  pickedIndices: Set<number>;
}

export function ExportOptionsModal({
  open,
  folder,
  assetTuidHex,
  assetName,
  hasSkeleton,
  primaryAnimsetHash,
  // initialExtraPicks intentionally unused — animations always start
  // unselected, see the listAnimsets effect below.
  initialExtraPicks: _initialExtraPicks,
  onClose,
  onExported,
}: ExportOptionsModalProps) {
  const [step, setStep] = useState<StepId>("scope");
  const [includeMesh, setIncludeMesh] = useState(true);
  const [includeMaterials, setIncludeMaterials] = useState(true);
  const [includeArmature, setIncludeArmature] = useState(hasSkeleton);
  const [textureQuality, setTextureQuality] = useState<"low" | "medium" | "high" | "original">("high");

  const [animsets, setAnimsets] = useState<AnimsetSummary[] | null>(null);
  const [animsetsError, setAnimsetsError] = useState<string | null>(null);
  const [picks, setPicks] = useState<Record<string, PerAnimsetSelection>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [savingError, setSavingError] = useState<string | null>(null);
  const [savingStatus, setSavingStatus] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setStep("scope");
    setIncludeMesh(true);
    setIncludeMaterials(true);
    setIncludeArmature(hasSkeleton);
    setSavingError(null);
    setSavingStatus(null);
  }, [open, hasSkeleton]);

  useEffect(() => {
    if (!open) return;
    if (animsets) return;
    listAnimsets(folder)
      .then((list) => {
        setAnimsets(list);
        // Start with no animation clips selected. The user explicitly
        // ticks the ones they want included in the export — avoids
        // accidentally bloating GLBs with the asset's full animset
        // when they only wanted geometry.
        setPicks({});
      })
      .catch((e) => setAnimsetsError(`${e}`));
  }, [open, folder, animsets]);

  const totalClipsPicked = useMemo(() => {
    return Object.values(picks).reduce(
      (acc, sel) => acc + sel.pickedIndices.size,
      0,
    );
  }, [picks]);

  const togglePick = (hash: string, idx: number) => {
    setPicks((prev) => {
      const cur = prev[hash] ?? { hash, pickedIndices: new Set<number>() };
      const next = new Set(cur.pickedIndices);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return { ...prev, [hash]: { hash, pickedIndices: next } };
    });
  };

  const toggleAllInAnimset = (hash: string, count: number) => {
    setPicks((prev) => {
      const cur = prev[hash];
      const allPicked = cur && cur.pickedIndices.size === count;
      return {
        ...prev,
        [hash]: {
          hash,
          pickedIndices: allPicked
            ? new Set<number>()
            : new Set(Array.from({ length: count }, (_, i) => i)),
        },
      };
    });
  };

  const sortedAnimsets = useMemo(() => {
    if (!animsets) return [];
    const lc = (s: string) => s.toLowerCase();
    const primary = primaryAnimsetHash ? lc(primaryAnimsetHash) : null;
    return [...animsets].sort((a, b) => {
      const aPrimary = primary && lc(a.hash) === primary ? 0 : 1;
      const bPrimary = primary && lc(b.hash) === primary ? 0 : 1;
      if (aPrimary !== bPrimary) return aPrimary - bPrimary;
      return a.hash.localeCompare(b.hash);
    });
  }, [animsets, primaryAnimsetHash]);

  const runExport = async () => {
    setSavingError(null);
    let path: string | null = null;
    try {
      path = await saveDialog({
        defaultPath: `${assetName.replace(/[\\/:"*?<>|]/g, "_") || assetTuidHex}.glb`,
        filters: [{ name: "Binary glTF", extensions: ["glb"] }],
        title: "Export .glb",
      });
    } catch (e) {
      setSavingError(`Save dialog failed: ${e}`);
      return;
    }
    if (!path) return;

    const extra_clips: ClipPick[] = Object.values(picks)
      .filter((sel) => sel.pickedIndices.size > 0)
      .map((sel) => ({
        animset_hash: sel.hash,
        clip_indices: [...sel.pickedIndices].sort((a, b) => a - b),
      }));

    const textureMaxDim =
      textureQuality === "low"
        ? 256
        : textureQuality === "medium"
          ? 512
          : textureQuality === "high"
            ? 1024
            : 0xffffffff;
    const options: GlbExportOptions = {
      include_mesh: includeMesh,
      include_materials: includeMaterials,
      include_armature: includeArmature,
      extra_clips,
      texture_max_dim: textureMaxDim,
    };

    setStep("saving");
    const canCopyCached =
      extra_clips.length === 0 &&
      includeMesh &&
      includeMaterials &&
      includeArmature &&
      textureMaxDim === 0xffffffff;

    try {
      let bytes: number;
      if (canCopyCached) {
        setSavingStatus("Copying cached GLB…");
        bytes = await exportCachedMobyGlb(folder, assetTuidHex, path);
      } else {
        setSavingStatus("Building GLB…");
        bytes = await exportMobyGlbWithOptions(folder, assetTuidHex, path, options);
      }
      setSavingStatus(`Exported ${bytes.toLocaleString()} bytes → ${path}`);
      onExported?.(path, bytes);
    } catch (e) {
      setSavingError(`Export failed: ${e}`);
      setStep("anims");
    }
  };

  const stepLabel = step === "scope" ? "1 / 2" : step === "anims" ? "2 / 2" : "Exporting";

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Export GLB"
      subtitle={
        <span className="mono small dim">
          {assetName || assetTuidHex} · step {stepLabel}
        </span>
      }
      size="lg"
      footer={
        step === "scope" ? (
          <div className="export-modal-footer">
            <button type="button" className="btn" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className="btn primary"
              onClick={() => setStep("anims")}
            >
              Next
            </button>
          </div>
        ) : step === "anims" ? (
          <div className="export-modal-footer">
            <button type="button" className="btn" onClick={() => setStep("scope")}>
              Back
            </button>
            <button type="button" className="btn" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className="btn primary"
              onClick={runExport}
            >
              Export ({totalClipsPicked} clip{totalClipsPicked === 1 ? "" : "s"})
            </button>
          </div>
        ) : (
          <div className="export-modal-footer">
            <button type="button" className="btn" onClick={onClose}>
              Close
            </button>
          </div>
        )
      }
    >
      {step === "scope" && (
        <div className="export-step">
          <p className="dim small">
            What should the GLB contain?
          </p>
          <label className="export-toggle">
            <input
              type="checkbox"
              checked={includeMesh}
              onChange={(e) => setIncludeMesh(e.target.checked)}
            />
            <span>
              <strong>Mesh</strong>
              <span className="dim small"> · vertex / index buffers</span>
            </span>
          </label>
          <label className="export-toggle">
            <input
              type="checkbox"
              checked={includeMaterials}
              onChange={(e) => setIncludeMaterials(e.target.checked)}
            />
            <span>
              <strong>Materials &amp; textures</strong>
              <span className="dim small"> · embedded PNGs</span>
            </span>
          </label>
          {includeMaterials && (
            <div className="export-quality-row">
              <label className="dim small">Texture quality</label>
              <div className="export-quality-options">
                {(
                  [
                    { id: "low" as const, label: "Low", hint: "256 px" },
                    { id: "medium" as const, label: "Medium", hint: "512 px" },
                    { id: "high" as const, label: "High", hint: "1024 px" },
                    { id: "original" as const, label: "Original", hint: "source res" },
                  ] as const
                ).map((q) => (
                  <button
                    key={q.id}
                    type="button"
                    className={`export-quality-btn${textureQuality === q.id ? " is-active" : ""}`}
                    onClick={() => setTextureQuality(q.id)}
                  >
                    <span>{q.label}</span>
                    <span className="dim small">{q.hint}</span>
                  </button>
                ))}
              </div>
              {textureQuality === "original" && (
                <p className="dim small">
                  Re-extracts from <code>textures.dat</code> at full source resolution. Slightly slower export.
                </p>
              )}
              {textureQuality !== "original" && textureQuality !== "low" && textureQuality !== "medium" && (
                <p className="dim small">
                  High pulls fresh from <code>textures.dat</code>. Low/Medium reuse the cached PNGs.
                </p>
              )}
            </div>
          )}
          <label className="export-toggle">
            <input
              type="checkbox"
              checked={includeArmature}
              onChange={(e) => setIncludeArmature(e.target.checked)}
              disabled={!hasSkeleton}
            />
            <span>
              <strong>Armature</strong>
              <span className="dim small">
                {hasSkeleton
                  ? " · bones + inverse bind matrices (required for animations)"
                  : " · this asset has no skeleton"}
              </span>
            </span>
          </label>
        </div>
      )}

      {step === "anims" && (
        <div className="export-step">
          <p className="dim small">
            Pick which animations to embed as Actions. The character's own animset
            is listed first; everything below it lives in other animsets you can mix
            in. Requires <em>Armature</em>.
          </p>
          {!includeArmature && (
            <p className="warn-text small">
              Armature is disabled — animations will be ignored on export.
            </p>
          )}
          {animsetsError && (
            <p className="warn-text small">Failed to list animsets: {animsetsError}</p>
          )}
          {!animsets && !animsetsError && (
            <p className="dim small">Loading animsets…</p>
          )}
          {animsets && (
            <ul className="export-animset-list">
              {sortedAnimsets.map((a) => {
                const isPrimary =
                  primaryAnimsetHash &&
                  a.hash.toLowerCase() === primaryAnimsetHash.toLowerCase();
                const selection = picks[a.hash];
                const pickedCount = selection?.pickedIndices.size ?? 0;
                const isExpanded = expanded[a.hash] ?? !!isPrimary;
                return (
                  <li
                    key={a.hash}
                    className={`export-animset${isPrimary ? " is-primary" : ""}`}
                  >
                    <div className="export-animset-header">
                      <button
                        type="button"
                        className="export-animset-toggle"
                        onClick={() =>
                          setExpanded((p) => ({ ...p, [a.hash]: !isExpanded }))
                        }
                      >
                        {isExpanded ? "▾" : "▸"}
                      </button>
                      <span className="export-animset-title mono small">
                        {a.hash}
                        {isPrimary && (
                          <span className="export-tag dim small"> · default</span>
                        )}
                      </span>
                      <span className="export-animset-count dim small">
                        {pickedCount} / {a.clips.length} picked
                      </span>
                      <button
                        type="button"
                        className="btn small"
                        onClick={() => toggleAllInAnimset(a.hash, a.clips.length)}
                        disabled={a.clips.length === 0}
                      >
                        {pickedCount === a.clips.length && a.clips.length > 0
                          ? "None"
                          : "All"}
                      </button>
                    </div>
                    {isExpanded && (
                      <ul className="export-clip-list">
                        {a.clips.length === 0 && (
                          <li className="dim small">no clips</li>
                        )}
                        {a.clips.map((c, i) => {
                          const checked = selection?.pickedIndices.has(i) ?? false;
                          return (
                            <li key={i}>
                              <label className="export-clip-row">
                                <input
                                  type="checkbox"
                                  checked={checked}
                                  onChange={() => togglePick(a.hash, i)}
                                />
                                <span className="export-clip-name mono small">
                                  {c.name || `(unnamed #${i})`}
                                </span>
                                <span className="export-clip-meta dim small">
                                  {c.num_frames}f @ {c.frame_rate.toFixed(0)}fps
                                  {c.looping ? " · loop" : ""}
                                </span>
                              </label>
                            </li>
                          );
                        })}
                      </ul>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}

      {step === "saving" && (
        <div className="export-step">
          {savingStatus && <p className="small">{savingStatus}</p>}
          {savingError && <p className="warn-text small">{savingError}</p>}
        </div>
      )}
    </Modal>
  );
}
