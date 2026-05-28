import { useCallback, useEffect, useMemo, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  assetLookupExtractStream,
  assetLookupInspect,
  type AssetLookupKindDto,
  type AssetLookupOverviewDto,
} from "../api";

type KindProgress = {
  total: number;
  current: number;
  lastFile: string;
  failures: number;
};

const ALL_KIND_ORDER = [
  "texture",
  "highmip",
  "cubemap",
  "moby",
  "tie",
  "shader",
  "zone",
  "animset",
  "foliage",
  "shrub",
  "cinematic",
  "lighting",
];

const DEFAULT_SELECTED = new Set<string>(["texture", "moby", "tie"]);

interface AssetLookupToolsProps {
  /** Notifies the host (modal) when extraction starts or ends so the modal
   *  can lock backdrop-dismissal while work is in progress. */
  onBusyChange?: (busy: boolean) => void;
}

export function AssetLookupTools({ onBusyChange }: AssetLookupToolsProps = {}) {
  const [inputPath, setInputPath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [overview, setOverview] = useState<AssetLookupOverviewDto | null>(null);
  const [selected, setSelected] = useState<Set<string>>(
    new Set(DEFAULT_SELECTED),
  );
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    onBusyChange?.(busy);
  }, [busy, onBusyChange]);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<Map<string, KindProgress>>(
    new Map(),
  );

  const handleBrowseInput = useCallback(async () => {
    try {
      const picked = await openDialog({
        directory: false,
        multiple: false,
        title: "Pick assetlookup.dat",
        filters: [
          { name: "Asset lookup", extensions: ["dat", "DAT"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof picked === "string") setInputPath(picked);
    } catch (e) {
      setError(`File picker failed: ${e}`);
    }
  }, []);

  const handleBrowseOutput = useCallback(async () => {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Pick an output folder",
      });
      if (typeof picked === "string") setOutputPath(picked);
    } catch (e) {
      setError(`Folder picker failed: ${e}`);
    }
  }, []);

  const handleInspect = useCallback(async () => {
    if (!inputPath.trim()) return;
    setBusy(true);
    setError(null);
    setProgress(new Map());
    try {
      const info = await assetLookupInspect(inputPath.trim());
      setOverview(info);
    } catch (e) {
      setError(String(e));
      setOverview(null);
    } finally {
      setBusy(false);
    }
  }, [inputPath]);

  const sortedKinds = useMemo<AssetLookupKindDto[]>(() => {
    if (!overview) return [];
    const byName = new Map(overview.kinds.map((k) => [k.name, k]));
    return ALL_KIND_ORDER.map((n) => byName.get(n)).filter(
      (k): k is AssetLookupKindDto => Boolean(k),
    );
  }, [overview]);

  const toggleKind = useCallback((name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }, []);

  const handleSelectAllDecoded = useCallback(() => {
    if (!overview) return;
    const next = new Set<string>();
    for (const k of overview.kinds) {
      if (k.has_decoder && k.count > 0) next.add(k.name);
    }
    setSelected(next);
  }, [overview]);

  const handleClearSelection = useCallback(() => {
    setSelected(new Set());
  }, []);

  const handleExtract = useCallback(async () => {
    if (!inputPath.trim() || !outputPath.trim() || selected.size === 0) return;
    setBusy(true);
    setError(null);
    setProgress(new Map());

    const kinds = ALL_KIND_ORDER.filter((n) => selected.has(n));

    try {
      await assetLookupExtractStream(
        inputPath.trim(),
        outputPath.trim(),
        kinds,
        null,
        (ev) => {
          switch (ev.type) {
            case "total":
              setProgress((prev) => {
                const next = new Map(prev);
                next.set(ev.kind, {
                  total: ev.count,
                  current: 0,
                  lastFile: "",
                  failures: 0,
                });
                return next;
              });
              break;
            case "entry":
              setProgress((prev) => {
                const next = new Map(prev);
                const cur = next.get(ev.kind) ?? {
                  total: 0,
                  current: 0,
                  lastFile: "",
                  failures: 0,
                };
                next.set(ev.kind, {
                  total: cur.total,
                  current: ev.index,
                  lastFile: ev.ok ? ev.tuid : `${ev.tuid} ✕`,
                  failures: cur.failures + (ev.ok ? 0 : 1),
                });
                return next;
              });
              break;
            case "kind_done":
              break;
            case "done":
              break;
            case "error":
              setError(ev.message);
              break;
          }
        },
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [inputPath, outputPath, selected]);

  return (
    <div className="psarc-tools">
      <div className="psarc-controls">
        <div className="psarc-row">
          <label className="psarc-label">assetlookup.dat</label>
          <input
            type="text"
            value={inputPath}
            onChange={(e) => setInputPath(e.target.value)}
            placeholder="Click Browse… or paste a path to assetlookup.dat"
            spellCheck={false}
            disabled={busy}
          />
          <button
            className="btn"
            onClick={handleBrowseInput}
            disabled={busy}
            title="Pick assetlookup.dat via the OS dialog"
          >
            Browse…
          </button>
          <button
            className="btn"
            onClick={handleInspect}
            disabled={busy || !inputPath.trim()}
          >
            Inspect
          </button>
        </div>
        <div className="psarc-row">
          <label className="psarc-label">Output folder</label>
          <input
            type="text"
            value={outputPath}
            onChange={(e) => setOutputPath(e.target.value)}
            placeholder="Click Browse… or paste a destination folder"
            spellCheck={false}
            disabled={busy}
          />
          <button
            className="btn"
            onClick={handleBrowseOutput}
            disabled={busy}
            title="Pick an output folder via the OS dialog"
          >
            Browse…
          </button>
          <button
            className="btn btn-primary"
            onClick={handleExtract}
            disabled={
              busy ||
              !inputPath.trim() ||
              !outputPath.trim() ||
              selected.size === 0 ||
              !overview
            }
          >
            {busy ? "Extracting…" : "Extract selected"}
          </button>
        </div>
      </div>

      {error && <div className="error-banner">{error}</div>}

      {overview && (
        <div className="psarc-info">
          <div className="psarc-meta">
            <span>
              <span className="dim">layout</span> {overview.layout}
            </span>
            <span>
              <span className="dim">v</span>
              {overview.version_major}.{overview.version_minor}
            </span>
            <span>
              <button
                className="btn btn-small"
                onClick={handleSelectAllDecoded}
                disabled={busy}
                title="Select every kind that has a decoder and a non-zero count"
              >
                Select all decoded
              </button>
            </span>
            <span>
              <button
                className="btn btn-small"
                onClick={handleClearSelection}
                disabled={busy}
              >
                Clear
              </button>
            </span>
          </div>

          <div className="psarc-table-wrap">
            <table className="psarc-table">
              <thead>
                <tr>
                  <th style={{ width: 34 }}></th>
                  <th>Kind</th>
                  <th style={{ width: 90, textAlign: "right" }}>Count</th>
                  <th style={{ width: 80 }}>Output</th>
                  <th>Progress</th>
                </tr>
              </thead>
              <tbody>
                {sortedKinds.map((k) => {
                  const isSelected = selected.has(k.name);
                  const prog = progress.get(k.name);
                  const pct =
                    prog && prog.total > 0
                      ? Math.min(100, (prog.current / prog.total) * 100)
                      : 0;
                  const outputLabel = outputFormatLabel(k.name, k.has_decoder);
                  const disabled = k.count === 0;
                  return (
                    <tr
                      key={k.name}
                      style={{ opacity: disabled ? 0.45 : 1 }}
                    >
                      <td>
                        <input
                          type="checkbox"
                          checked={isSelected}
                          disabled={busy || disabled}
                          onChange={() => toggleKind(k.name)}
                        />
                      </td>
                      <td className="mono small">
                        {k.name}
                        {!k.has_decoder && (
                          <span
                            className="dim small"
                            style={{ marginLeft: 6 }}
                          >
                            (table only)
                          </span>
                        )}
                      </td>
                      <td
                        className="mono small"
                        style={{ textAlign: "right" }}
                      >
                        {k.count.toLocaleString()}
                      </td>
                      <td className="mono small dim">{outputLabel}</td>
                      <td>
                        {prog ? (
                          <div className="psarc-progress">
                            <div className="load-progress-bar">
                              <div
                                className="load-progress-fill"
                                style={{ width: `${pct}%` }}
                              />
                            </div>
                            <div className="psarc-progress-meta">
                              <span className="mono small">
                                {prog.current.toLocaleString()} /{" "}
                                {prog.total.toLocaleString()}
                                {prog.failures > 0 && (
                                  <span
                                    className="dim small"
                                    style={{ marginLeft: 6 }}
                                  >
                                    ({prog.failures} failed)
                                  </span>
                                )}
                              </span>
                              <span
                                className="mono small dim"
                                style={{
                                  overflow: "hidden",
                                  textOverflow: "ellipsis",
                                  whiteSpace: "nowrap",
                                  maxWidth: 220,
                                }}
                              >
                                {prog.lastFile}
                              </span>
                            </div>
                          </div>
                        ) : (
                          <span className="dim small">—</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {!overview && !error && (
        <div className="tree-empty">
          Pick an <code>assetlookup.dat</code> from an R2 / R3 / RCFFA level
          folder and click <strong>Inspect</strong>.
          <p className="dim small" style={{ marginTop: 6 }}>
            Decodes textures → PNG, mobys/ties → GLB, shaders/zones → JSON.
            Other kinds emit a pointer-table JSON. RFOM and TOD layouts are
            not yet supported here.
          </p>
        </div>
      )}
    </div>
  );
}

function outputFormatLabel(kind: string, hasDecoder: boolean): string {
  switch (kind) {
    case "texture":
    case "highmip":
      return ".png";
    case "cubemap":
      return "6×.png";
    case "moby":
    case "tie":
      return ".glb";
    case "shader":
    case "zone":
      return ".json";
    default:
      return hasDecoder ? "—" : "table.json";
  }
}
