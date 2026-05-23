import { AlertTriangle, Database, Loader2 } from "lucide-react";
import type { CacheStatus, LevelMeshes, LevelSummary } from "../api";
import type { LoadPhaseState } from "../components/LoadProgress";

interface StatusBarProps {
  summary: LevelSummary | null;
  meshesCount: number;
  instanceCount: number;
  ufragCount: number;
  meshes: LevelMeshes | null;
  loadPhase: LoadPhaseState | null;
  meshLoadPhase?: LoadPhaseState | null;
  error: string | null;
  cacheState?: CacheStatus | null;
  cacheProgress?: {
    phase: "mobys" | "ties" | "materials" | "normalmaps" | "textures" | "ufrags";
    current: number;
    total: number;
  } | null;
  


  onOpenCacheLibrary?: () => void;
}

function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}






export function StatusBar({
  summary,
  meshesCount,
  instanceCount,
  ufragCount,
  meshes,
  loadPhase,
  meshLoadPhase,
  error,
  cacheState,
  cacheProgress,
  onOpenCacheLibrary,
}: StatusBarProps) {
  const activePhase = loadPhase ?? meshLoadPhase ?? null;
  const dotClass = error
    ? "error"
    : activePhase
      ? "busy"
      : summary
        ? "ok"
        : "";

  const dotLabel = error
    ? "Error"
    : activePhase
      ? activePhase.label
      : summary
        ? "Ready"
        : "Idle";

  return (
    <div className="statusbar">
      <span className="statusbar-cell">
        <span className={`statusbar-dot ${dotClass}`} />
        <span>{dotLabel}</span>
      </span>

      {summary && (
        <>
          <span className="statusbar-divider" />
          <span className="statusbar-cell" title={summary.folder}>
            {folderName(summary.folder)}
          </span>
          <span className="statusbar-divider" />
          <span className="statusbar-cell">
            IGHW v{summary.version_major}.{summary.version_minor}
          </span>
          <span className="statusbar-divider" />
          <span className="statusbar-cell">
            {summary.sections.length} sections
          </span>
        </>
      )}

      <span className="toolbar-spacer" />

      {summary && (
        <>
          <span className="statusbar-cell">
            {instanceCount.toLocaleString()} instances
          </span>
          <span className="statusbar-divider" />
          <span className="statusbar-cell">
            {ufragCount.toLocaleString()} UFrags
          </span>
          {meshes && (
            <>
              <span className="statusbar-divider" />
              <span className="statusbar-cell">
                {meshes.moby_assets.length}M / {meshes.tie_assets.length}T /{" "}
                {meshes.ufrag_meshes.length}U
              </span>
              <span className="statusbar-divider" />
              <span className="statusbar-cell">
                {meshes.textures.length} tex
              </span>
            </>
          )}
          {cacheProgress && (
            <>
              <span className="statusbar-divider" />
              <span
                className="statusbar-cell"
                title={`Caching ${cacheProgress.phase} ${cacheProgress.current}/${cacheProgress.total}`}
              >
                <Loader2 size={11} className="spin" />
                Caching {cacheProgress.phase} {cacheProgress.current}/
                {cacheProgress.total}
              </span>
            </>
          )}
          {!cacheProgress && cacheState?.exists && (
            <>
              <span className="statusbar-divider" />
              {onOpenCacheLibrary ? (
                <button
                  type="button"
                  className={`statusbar-cell statusbar-cell-button ${cacheState.stale ? "stale" : ""}`}
                  onClick={onOpenCacheLibrary}
                  title={
                    cacheState.stale
                      ? `Cache stale — source files have changed since last extract. Re-extract from the library modal. (${cacheState.cache_path})`
                      : `Browse cache · ${cacheState.cache_path}`
                  }
                >
                  {cacheState.stale ? (
                    <AlertTriangle size={11} />
                  ) : (
                    <Database size={11} />
                  )}
                  {cacheState.mobys}M / {cacheState.ties}T /{" "}
                  {cacheState.textures}tex
                  {cacheState.stale ? " · stale" : " cached"}
                </button>
              ) : (
                <span
                  className="statusbar-cell"
                  title={`Cache: ${cacheState.cache_path}`}
                >
                  <Database size={11} />
                  {cacheState.mobys}M / {cacheState.ties}T /{" "}
                  {cacheState.textures}tex cached
                </span>
              )}
            </>
          )}
        </>
      )}
      {}
      {meshesCount < 0 && null}
    </div>
  );
}
