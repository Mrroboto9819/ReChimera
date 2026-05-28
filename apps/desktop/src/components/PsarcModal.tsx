import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Archive, Folder, X } from "lucide-react";
import {
  psarcExtractStream,
  psarcList,
  type PsarcListDto,
} from "../api";
import { Modal } from "./Modal";
import { Button } from "../ui";
import { useFileDrop } from "../useFileDrop";

interface PsarcModalProps {
  open: boolean;
  onClose: () => void;
}

interface ExtractStatus {
  total: number;
  current: number;
  lastFile: string;
}

const RECENT_KEY = "rechimera.recentPsarc";
const RECENT_MAX = 6;

function loadRecent(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((s) => typeof s === "string")
      : [];
  } catch {
    return [];
  }
}

function pushRecent(path: string): string[] {
  const current = loadRecent().filter((p) => p !== path);
  const next = [path, ...current].slice(0, RECENT_MAX);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    
  }
  return next;
}

function lastTwoSegments(path: string): string {
  const norm = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const parts = norm.split("/");
  return parts.slice(-2).join(" / ") || norm;
}

/// Drop-target accept: any path ending in .psarc OR a folder. The
/// caller distinguishes archive vs output folder by extension.
function acceptPsarcDrop(p: string): boolean {
  if (p.endsWith(".psarc")) return true;
  return !/\.[a-z0-9]{1,6}$/i.test(p);
}







export function PsarcModal({ open, onClose }: PsarcModalProps) {
  const [inputPath, setInputPath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [info, setInfo] = useState<PsarcListDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [progress, setProgress] = useState<ExtractStatus | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const totalRef = useRef(0);

  useEffect(() => {
    if (open) {
      setRecent(loadRecent());
      setError(null);
      setWarning(null);
    }
  }, [open]);

  const handleBrowseInput = useCallback(async () => {
    setWarning(null);
    try {
      const picked = await openDialog({
        directory: false,
        multiple: false,
        title: "Pick a .psarc archive",
        filters: [
          { name: "PlayStation Archive", extensions: ["psarc", "PSARC"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof picked === "string") setInputPath(picked);
    } catch (e) {
      setError(`File picker failed: ${e}`);
    }
  }, []);

  const handleBrowseOutput = useCallback(async () => {
    setWarning(null);
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

  
  
  const handleDrop = useCallback((paths: string[]) => {
    if (paths.length === 0) {
      setWarning("Drop a .psarc file or an output folder.");
      return;
    }
    setWarning(null);
    for (const p of paths) {
      if (p.toLowerCase().endsWith(".psarc")) {
        setInputPath(p);
      } else {
        setOutputPath(p);
      }
    }
  }, []);

  const dropPhase = useFileDrop({
    enabled: open && !busy,
    accept: acceptPsarcDrop,
    onDrop: handleDrop,
  });

  const handleList = useCallback(async () => {
    if (!inputPath.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const list = await psarcList(inputPath.trim());
      setInfo(list);
    } catch (e) {
      setError(String(e));
      setInfo(null);
    } finally {
      setBusy(false);
    }
  }, [inputPath]);

  const handleExtract = useCallback(async () => {
    if (!inputPath.trim() || !outputPath.trim()) return;
    setBusy(true);
    setError(null);
    totalRef.current = 0;
    setProgress({ total: 0, current: 0, lastFile: "" });
    try {
      await psarcExtractStream(inputPath.trim(), outputPath.trim(), (e) => {
        switch (e.type) {
          case "total":
            totalRef.current = e.total;
            setProgress({ total: e.total, current: 0, lastFile: "" });
            break;
          case "file":
            setProgress({
              total: totalRef.current,
              current: e.index,
              lastFile: e.name,
            });
            break;
          case "done":
            setProgress((p) =>
              p ? { ...p, current: p.total, lastFile: "Done." } : p,
            );
            pushRecent(inputPath.trim());
            setRecent(loadRecent());
            break;
          case "error":
            setError(e.message);
            break;
        }
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [inputPath, outputPath]);

  const removeRecent = useCallback((path: string) => {
    const next = loadRecent().filter((p) => p !== path);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(next));
    } catch {
      
    }
    setRecent(next);
  }, []);

  const filteredEntries = info?.entries.filter((e) =>
    filter ? e.name.toLowerCase().includes(filter.toLowerCase()) : true,
  );

  const pct =
    progress && progress.total > 0
      ? Math.min(100, (progress.current / progress.total) * 100)
      : 0;

  return (
    <Modal
      open={open}
      onClose={onClose}
      dismissable={!busy}
      title="Extract PSARC"
      subtitle="Pick a .psarc archive and a destination folder"
      size="lg"
      footer={
        <>
          <Button onClick={onClose} disabled={busy}>
            Close
          </Button>
          <Button
            onClick={handleList}
            disabled={busy || !inputPath.trim()}
            loading={busy && !progress}
          >
            List files
          </Button>
          <Button
            variant="primary"
            onClick={handleExtract}
            disabled={!inputPath.trim() || !outputPath.trim()}
            loading={busy && !!progress}
          >
            {busy && progress ? "Extracting…" : "Extract all"}
          </Button>
        </>
      }
    >
      <div className={`open-level ${dropPhase === "over" ? "drop-over" : ""}`}>
        <div className="open-level-droptarget">
          <div className="open-level-droptarget-text">
            {dropPhase === "over"
              ? "Drop .psarc here (or a folder for the output)"
              : "Drag a .psarc file or output folder here"}
          </div>
        </div>

        <div className="open-level-pickers">
          <button
            type="button"
            className="open-level-card"
            onClick={handleBrowseInput}
            disabled={busy}
          >
            <div className="open-level-card-icon" aria-hidden>
              <Archive size={28} strokeWidth={1.5} />
            </div>
            <div className="open-level-card-text">
              <div className="open-level-card-title">
                Pick <code>.psarc</code>
              </div>
              <div className="open-level-card-sub small dim">
                ZLIB-compressed PSAR v1.3 / v1.4
              </div>
            </div>
          </button>

          <button
            type="button"
            className="open-level-card"
            onClick={handleBrowseOutput}
            disabled={busy}
          >
            <div className="open-level-card-icon" aria-hidden>
              <Folder size={28} strokeWidth={1.5} />
            </div>
            <div className="open-level-card-text">
              <div className="open-level-card-title">Pick output folder</div>
              <div className="open-level-card-sub small dim">
                Destination for extracted files
              </div>
            </div>
          </button>
        </div>

        <label className="open-level-field">
          <span className="open-level-field-label small dim">
            Or paste paths
          </span>
          <input
            type="text"
            value={inputPath}
            onChange={(e) => setInputPath(e.target.value)}
            placeholder="C:\\path\\to\\archive.psarc"
            spellCheck={false}
            disabled={busy}
          />
        </label>
        <label className="open-level-field">
          <input
            type="text"
            value={outputPath}
            onChange={(e) => setOutputPath(e.target.value)}
            placeholder="C:\\path\\to\\output"
            spellCheck={false}
            disabled={busy}
          />
        </label>

        {warning && <div className="open-level-warning">{warning}</div>}
        {error && <div className="error-banner">{error}</div>}

        {progress && progress.total > 0 && (
          <div className="psarc-progress" style={{ marginTop: 4 }}>
            <div className="load-progress-bar">
              <div
                className="load-progress-fill"
                style={{ width: `${pct}%` }}
              />
            </div>
            <div className="psarc-progress-meta">
              <span className="mono small">
                {progress.current.toLocaleString()} /{" "}
                {progress.total.toLocaleString()}
              </span>
              <span
                className="mono small dim"
                style={{
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  maxWidth: "60%",
                }}
              >
                {progress.lastFile}
              </span>
            </div>
          </div>
        )}

        {info && (
          <div className="psarc-info" style={{ marginTop: 4 }}>
            <div className="psarc-meta">
              <span>
                <span className="dim">v</span>
                {info.major}.{info.minor}
              </span>
              <span>
                <span className="dim">comp</span> {info.compression}
              </span>
              <span>
                <span className="dim">block</span>{" "}
                {info.block_size.toLocaleString()}B
              </span>
              <span>
                <strong>{info.entry_count.toLocaleString()}</strong> files
              </span>
            </div>
            <div className="psarc-search">
              <input
                type="text"
                placeholder="Filter files…"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                spellCheck={false}
              />
            </div>
            <div className="psarc-table-wrap" style={{ maxHeight: 220 }}>
              <table className="psarc-table">
                <thead>
                  <tr>
                    <th>Path</th>
                    <th style={{ width: 90, textAlign: "right" }}>Size</th>
                  </tr>
                </thead>
                <tbody>
                  {(filteredEntries ?? []).slice(0, 1000).map((e, i) => (
                    <tr key={`${e.name}-${i}`}>
                      <td className="mono small">
                        {e.name || <em>unnamed</em>}
                      </td>
                      <td
                        className="mono small"
                        style={{ textAlign: "right" }}
                      >
                        {e.uncompressed_size.toLocaleString()}
                      </td>
                    </tr>
                  ))}
                  {(filteredEntries?.length ?? 0) > 1000 && (
                    <tr>
                      <td colSpan={2} className="dim small" style={{ padding: 6 }}>
                        showing first 1000 of{" "}
                        {(filteredEntries?.length ?? 0).toLocaleString()}
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>
        )}

        {recent.length > 0 && (
          <div className="open-level-recent">
            <div className="open-level-section-title small dim">Recent</div>
            <ul className="open-level-recent-list">
              {recent.map((p) => (
                <li key={p} className="open-level-recent-item">
                  <button
                    type="button"
                    className="open-level-recent-btn"
                    onClick={() => setInputPath(p)}
                    disabled={busy}
                    title={p}
                  >
                    <span className="open-level-recent-name">
                      {lastTwoSegments(p)}
                    </span>
                    <span className="open-level-recent-path mono small dim">
                      {p}
                    </span>
                  </button>
                  <button
                    type="button"
                    className="open-level-recent-remove"
                    onClick={() => removeRecent(p)}
                    title="Remove from recent"
                    aria-label="Remove"
                  >
                    <X size={14} strokeWidth={2} />
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}

        <div className="open-level-hint small dim">
          Supports any ZLIB-compressed PSARC v1.3 or v1.4 archive (most
          PS3-era games). LZMA and OODLE compressions are recognized but
          not yet decoded.
        </div>
      </div>
    </Modal>
  );
}
