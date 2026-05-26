import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type {
  AssetKind,
  AssetMeshes,
  CacheManifest,
  CacheManifestEntry,
  Instance,
  SoundEntry,
} from "../api";
import { clickMods, type useSelection } from "../selection";

type Selection = ReturnType<typeof useSelection>;

interface HierarchyProps {
  instances: Instance[];
  selection: Selection;
  cacheManifest?: CacheManifest | null;
  sounds?: SoundEntry[];
  onPreviewRawAsset?: (assetTuid: string) => void;
  onSelectCacheSound?: (key: string) => void;
  onSelectCacheTexture?: (texId: string) => void;
}



interface AssetTreeNode {
  
  label: string;
  

  path: string;
  

  kind: AssetKind;
  
  asset?: AssetMeshes;
  

  children: AssetTreeNode[];
}












function buildAssetTree(
  rootLabel: string,
  rootKind: AssetKind,
  assets: AssetMeshes[],
): AssetTreeNode {
  const root: AssetTreeNode = {
    label: rootLabel,
    path: `__root_${rootKind}`,
    kind: rootKind,
    children: [],
  };
  
  
  
  
  
  
  const folderMap = new Map<string, AssetTreeNode>();
  for (const asset of assets) {
    
    
    
    const fullName = asset.name && asset.name.length > 0
      ? asset.name
      : asset.asset_tuid.slice(-12);
    const segments = fullName.split(/[/\\]+/).filter((s) => s.length > 0);
    if (segments.length === 0) {
      
      
      root.children.push({
        label: asset.asset_tuid.slice(-12),
        path: `${root.path}/${asset.asset_tuid}`,
        kind: rootKind,
        asset,
        children: [],
      });
      continue;
    }
    let cursor = root;
    for (let i = 0; i < segments.length; i++) {
      const seg = segments[i]!;
      const isLeaf = i === segments.length - 1;
      const childPath = `${cursor.path}/${seg}`;
      if (isLeaf) {
        cursor.children.push({
          label: seg,
          path: childPath,
          kind: rootKind,
          asset,
          children: [],
        });
      } else {
        let folder = folderMap.get(childPath);
        if (!folder) {
          folder = {
            label: seg,
            path: childPath,
            kind: rootKind,
            children: [],
          };
          cursor.children.push(folder);
          folderMap.set(childPath, folder);
        }
        cursor = folder;
      }
    }
  }
  
  const sortRecursive = (n: AssetTreeNode) => {
    n.children.sort((a, b) => {
      const aFolder = !a.asset;
      const bFolder = !b.asset;
      if (aFolder !== bFolder) return aFolder ? -1 : 1;
      return a.label.localeCompare(b.label);
    });
    for (const c of n.children) sortRecursive(c);
  };
  sortRecursive(root);
  return root;
}

interface Group {
  kind: AssetKind;
  label: string;
  instances: Instance[];
}


const KIND_LABELS: Partial<Record<AssetKind, string>> = {
  moby: "Mobys",
  tie: "Ties",
  detail: "Details",
  shrub: "Shrubs",
  foliage: "Foliage",
  light: "Lights",
  envsampler: "Env Probes",
  sky: "Skybox",
};

const KIND_GLYPHS: Partial<Record<AssetKind, string>> = {
  moby: "M",
  tie: "T",
  detail: "D",
  shrub: "F",
  foliage: "G",
  light: "L",
  envsampler: "E",
  sky: "S",
};










export function Hierarchy({
  instances,
  selection,
  cacheManifest,
  sounds,
  onPreviewRawAsset,
  onSelectCacheSound,
  onSelectCacheTexture,
}: HierarchyProps) {
  const [cacheGroupCollapsed, setCacheGroupCollapsed] = useState<{
    sounds: boolean;
    textures: boolean;
  }>({ sounds: true, textures: true });
  
  
  
  
  
  const [expandedKinds, setExpandedKinds] = useState<Set<AssetKind>>(new Set());
  const [assetLibCollapsed, setAssetLibCollapsed] = useState<
    Record<string, boolean>
  >({});
  // Per-asset toggle for the inline submesh + texture list under each
  // moby/tie leaf. Defaults to collapsed (44-submesh mobys would drown
  // the panel otherwise — opt-in per asset).
  const [expandedAssets, setExpandedAssets] = useState<Set<string>>(new Set());

  // Derive the set of texture IDs actually extracted to disk so we can
  // mark submesh texture refs that point at nothing with the same
  // purple marker the AssetWorkbench uses for missing-data tints.
  const cacheTextureIds = useMemo(() => {
    const out = new Set<number>();
    if (!cacheManifest) return out;
    for (const e of cacheManifest.entries) {
      if (e.kind !== "texture") continue;
      const stem = e.name?.replace(/\.png$/i, "") ?? "";
      const n = Number(stem);
      if (Number.isFinite(n)) out.add(n);
    }
    return out;
  }, [cacheManifest]);
  const [filter, setFilter] = useState("");
  const [section, setSection] = useState<"map" | "cache">(() => {
    try {
      const v = localStorage.getItem("rechimera.hierarchySection");
      if (v === "map" || v === "cache") return v;
    } catch {
      /* ignore */
    }
    return "map";
  });
  useEffect(() => {
    try {
      localStorage.setItem("rechimera.hierarchySection", section);
    } catch {
      /* ignore */
    }
  }, [section]);

  // Build the path-grouped tree once per asset list change. The
  // streaming pipeline emits a new array reference on every flush, so
  // raw `mobyAssets`/`tieAssets` deps would re-run this on every
  // chunk. `useDeferredValue` lets React schedule the rebuild as a
  // low-priority transition — the main render thread stays responsive
  // while the tree catches up. Combined with the O(n) Map-backed
  // `buildAssetTree`, this drops dense-level loads from 50-500ms of
  // cumulative jank to "barely measurable".

  
  
  
  
  
  
  const cacheTree = useMemo(() => {
    if (!cacheManifest || cacheManifest.entries.length === 0) return null;
    const cacheMobys: AssetMeshes[] = [];
    const cacheTies: AssetMeshes[] = [];
    const cacheDetails: AssetMeshes[] = [];
    const cacheSkies: AssetMeshes[] = [];
    const shellOf = (e: CacheManifestEntry): AssetMeshes => ({
      asset_tuid: e.tuid,
      name: e.name,
      submeshes: [],
      skeleton: null,
      animset_hash: null,
      bind_pose_inverse_offset: 0,
    });
    for (const entry of cacheManifest.entries) {
      if (entry.kind === "moby") {
        if (!cacheMobys.some((m) => m.asset_tuid === entry.tuid)) {
          cacheMobys.push(shellOf(entry));
        }
      } else if (entry.kind === "tie") {
        if (!cacheTies.some((t) => t.asset_tuid === entry.tuid)) {
          cacheTies.push(shellOf(entry));
        }
      } else if (entry.kind === "detail") {
        if (!cacheDetails.some((t) => t.asset_tuid === entry.tuid)) {
          cacheDetails.push(shellOf(entry));
        }
      } else if (entry.kind === "sky") {
        if (!cacheSkies.some((s) => s.asset_tuid === entry.tuid)) {
          cacheSkies.push(shellOf(entry));
        }
      }
    }
    if (
      cacheMobys.length === 0 &&
      cacheTies.length === 0 &&
      cacheDetails.length === 0 &&
      cacheSkies.length === 0
    )
      return null;
    const roots: AssetTreeNode[] = [];
    if (cacheMobys.length > 0)
      roots.push(buildAssetTree("Cache · Mobys", "moby", cacheMobys));
    if (cacheTies.length > 0)
      roots.push(buildAssetTree("Cache · Ties", "tie", cacheTies));
    if (cacheDetails.length > 0)
      roots.push(buildAssetTree("Cache · Details", "detail", cacheDetails));
    if (cacheSkies.length > 0)
      roots.push(buildAssetTree("Cache · Sky", "sky", cacheSkies));
    return roots;
  }, [cacheManifest]);

  
  
  
  
  
  
  
  
  
  
  
  
  
  
  const treeRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const primary = selection.primary;
    if (!primary || !treeRef.current) return;
    
    
    const safe = (window.CSS && window.CSS.escape ? window.CSS.escape(primary) : primary)
      .replace(/"/g, '\\"');
    const el = treeRef.current.querySelector<HTMLElement>(
      `[data-tuid="${safe}"]`,
    );
    if (el) {
      // `block: "nearest"` avoids jumpy scrolling when the row is
      // already partially visible; `behavior: "smooth"` is fine here
      // because the panel is short.
      el.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [selection.primary]);

  const groups: Group[] = useMemo(() => {
    const byKind = new Map<AssetKind, Instance[]>();
    for (const inst of instances) {
      let arr = byKind.get(inst.kind);
      if (!arr) {
        arr = [];
        byKind.set(inst.kind, arr);
      }
      arr.push(inst);
    }
    const out: Group[] = [];
    for (const [kind, list] of byKind) {
      out.push({
        kind,
        label: KIND_LABELS[kind] ?? kind,
        instances: list,
      });
    }
    out.sort((a, b) => a.label.localeCompare(b.label));
    return out;
  }, [instances]);

  const filterLowerForFlat = filter.trim().toLowerCase();
  const flatVisibleInstances = useMemo(() => {
    if (section !== "map") return [];
    const out: Instance[] = [];
    for (const g of groups) {
      if (!expandedKinds.has(g.kind)) continue;
      const list = filterLowerForFlat
        ? g.instances.filter((i) =>
            i.name.toLowerCase().includes(filterLowerForFlat),
          )
        : g.instances;
      for (const inst of list) out.push(inst);
    }
    return out;
  }, [section, groups, expandedKinds, filterLowerForFlat]);

  const moveSelection = useCallback(
    (direction: 1 | -1) => {
      if (flatVisibleInstances.length === 0) return;
      const currentIdx = selection.primary
        ? flatVisibleInstances.findIndex((i) => i.tuid === selection.primary)
        : -1;
      let nextIdx: number;
      if (currentIdx === -1) {
        nextIdx = direction === 1 ? 0 : flatVisibleInstances.length - 1;
      } else {
        nextIdx = Math.max(
          0,
          Math.min(flatVisibleInstances.length - 1, currentIdx + direction),
        );
      }
      const target = flatVisibleInstances[nextIdx];
      if (target && target.tuid !== selection.primary) {
        selection.select(target, { ctrl: false, shift: false });
      }
    },
    [flatVisibleInstances, selection],
  );

  useEffect(() => {
    if (section !== "map") return;
    const handler = (event: KeyboardEvent) => {
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
      const target = event.target as HTMLElement | null;
      if (target) {
        const tag = target.tagName;
        if (
          tag === "INPUT" ||
          tag === "TEXTAREA" ||
          tag === "SELECT" ||
          target.isContentEditable
        ) {
          return;
        }
      }
      const treeEl = treeRef.current;
      if (!treeEl) return;
      const root = treeEl.closest(".panel") ?? treeEl;
      if (!(root instanceof HTMLElement)) return;
      if (!root.matches(":hover") && !root.contains(document.activeElement)) {
        return;
      }
      event.preventDefault();
      moveSelection(event.key === "ArrowDown" ? 1 : -1);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [section, moveSelection]);

  const toggle = (kind: AssetKind) =>
    setExpandedKinds((prev) => {
      const next = new Set(prev);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });

  const filterLower = filter.trim().toLowerCase();

  const sections: {
    id: "map" | "cache";
    label: string;
    count: number;
  }[] = [
    { id: "map", label: "Map", count: instances.length },
    {
      id: "cache",
      label: "Cache",
      count: cacheManifest
        ? cacheManifest.entries.filter(
            (e) => e.kind === "moby" || e.kind === "tie",
          ).length
        : 0,
    },
  ];

  return (
    <div className="panel pane-hierarchy view-flush">
      <div className="hierarchy-subtabs" role="tablist">
        {sections.map((s) => (
          <button
            key={s.id}
            role="tab"
            aria-selected={section === s.id}
            className={`hierarchy-subtab ${section === s.id ? "active" : ""}`}
            onClick={() => setSection(s.id)}
          >
            <span>{s.label}</span>
            {s.count > 0 && (
              <span className="hierarchy-subtab-count">
                {s.count.toLocaleString()}
              </span>
            )}
          </button>
        ))}
      </div>
      <div className="hierarchy-search">
        <input
          type="text"
          placeholder="Filter…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          spellCheck={false}
        />
      </div>
      <div className="panel-body" ref={treeRef}>
        {section === "map" && (
          groups.length === 0 ? (
            <div className="tree-empty">No instances loaded</div>
          ) : (
            <div className="hierarchy-tree">
              {groups.map((g) => {
                const isCollapsed = !expandedKinds.has(g.kind);
                const filtered = filterLower
                  ? g.instances.filter((i) =>
                      i.name.toLowerCase().includes(filterLower),
                    )
                  : g.instances;
                return (
                  <div key={g.kind}>
                    <div className="tree-node" onClick={() => toggle(g.kind)}>
                      <span className="tree-toggle">{isCollapsed ? "▸" : "▾"}</span>
                      <span className={`tree-icon kind-${g.kind}`}>
                        {KIND_GLYPHS[g.kind] ?? "?"}
                      </span>
                      <span className="tree-label">{g.label}</span>
                      <span className="tree-count">
                        {filtered.length === g.instances.length
                          ? g.instances.length.toLocaleString()
                          : `${filtered.length}/${g.instances.length}`}
                      </span>
                    </div>
                    {!isCollapsed && (
                      <div className="tree-children">
                        {filtered.map((inst, idx) => (
                          <div
                            key={`${inst.tuid}-${idx}`}
                            data-tuid={inst.tuid}
                            className={`tree-node ${
                              selection.isSelected(inst.tuid) ? "selected" : ""
                            } ${selection.primary === inst.tuid ? "primary" : ""}`}
                            onClick={(e) => {
                              e.stopPropagation();
                              selection.select(inst, clickMods(e));
                            }}
                          >
                            <span className="tree-toggle" />
                            <span className={`tree-icon kind-${inst.kind}`}>
                              {KIND_GLYPHS[inst.kind] ?? "?"}
                            </span>
                            <span className="tree-label">
                              {inst.name || (
                                <span className="dim">unnamed</span>
                              )}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )
        )}

        {section === "cache" && (
          <>
            {cacheTree && cacheTree.length > 0 ? (
              <div className="hierarchy-tree">
                {cacheTree.map((root) => (
                  <AssetLibraryTree
                    key={`cache-${root.path}`}
                    node={root}
                    depth={0}
                    collapsed={assetLibCollapsed}
                    setCollapsed={setAssetLibCollapsed}
                    filter={filterLower}
                    instances={instances}
                    selection={selection}
                    onPreviewRawAsset={onPreviewRawAsset}
                    expandedAssets={expandedAssets}
                    setExpandedAssets={setExpandedAssets}
                    cacheTextureIds={cacheTextureIds}
                  />
                ))}
              </div>
            ) : (
              <div className="tree-empty">
                No cached assets yet — extract a level into _rechimera_cache/ to populate this list.
              </div>
            )}

            {(() => {
              const textureEntries = cacheManifest
                ? cacheManifest.entries.filter((e) => e.kind === "texture")
                : [];
              const filtered = filterLower
                ? textureEntries.filter(
                    (e) =>
                      e.tuid.toLowerCase().includes(filterLower) ||
                      e.file.toLowerCase().includes(filterLower),
                  )
                : textureEntries;
              if (textureEntries.length === 0) return null;
              const isCollapsed = cacheGroupCollapsed.textures;
              return (
                <div className="hierarchy-tree">
                  <div
                    className="tree-node"
                    onClick={() =>
                      setCacheGroupCollapsed((p) => ({
                        ...p,
                        textures: !p.textures,
                      }))
                    }
                  >
                    <span className="tree-toggle">
                      {isCollapsed ? "▸" : "▾"}
                    </span>
                    <span className="tree-icon kind-texture">▦</span>
                    <span className="tree-label">Cache · Textures</span>
                    <span className="tree-count">
                      {filtered.length === textureEntries.length
                        ? textureEntries.length.toLocaleString()
                        : `${filtered.length}/${textureEntries.length}`}
                    </span>
                  </div>
                  {!isCollapsed && (
                    <div className="tree-children">
                      {filtered.slice(0, 500).map((e) => {
                        const idMatch = e.file.match(/textures\/(\d+)\.png$/);
                        const texId = idMatch ? idMatch[1] : e.tuid;
                        return (
                          <div
                            key={e.file}
                            className="tree-node is-clickable"
                            onClick={() => {
                              const idMatch = e.file.match(
                                /textures\/(\d+)\.png$/,
                              );
                              const texId = idMatch ? idMatch[1] : e.tuid;
                              onSelectCacheTexture?.(texId!);
                            }}
                            title={`${(e.size_bytes / 1024).toFixed(0)} KB · click to preview / download`}
                          >
                            <span className="tree-toggle" />
                            <span className="tree-icon kind-texture">▦</span>
                            <span className="tree-label mono">{texId}</span>
                          </div>
                        );
                      })}
                      {filtered.length > 500 && (
                        <div className="tree-empty small dim">
                          + {filtered.length - 500} more — narrow with the filter
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })()}

            {(() => {
              if (!sounds || sounds.length === 0) return null;
              const filtered = filterLower
                ? sounds.filter(
                    (s) =>
                      s.name.toLowerCase().includes(filterLower) ||
                      s.source.toLowerCase().includes(filterLower),
                  )
                : sounds;
              const isCollapsed = cacheGroupCollapsed.sounds;
              return (
                <div className="hierarchy-tree">
                  <div
                    className="tree-node"
                    onClick={() =>
                      setCacheGroupCollapsed((p) => ({
                        ...p,
                        sounds: !p.sounds,
                      }))
                    }
                  >
                    <span className="tree-toggle">
                      {isCollapsed ? "▸" : "▾"}
                    </span>
                    <span className="tree-icon">♪</span>
                    <span className="tree-label">Cache · Sounds</span>
                    <span className="tree-count">
                      {filtered.length === sounds.length
                        ? sounds.length.toLocaleString()
                        : `${filtered.length}/${sounds.length}`}
                    </span>
                  </div>
                  {!isCollapsed && (
                    <div className="tree-children">
                      {filtered.slice(0, 500).map((s) => {
                        const key = `${s.source}-${s.index}-${s.name}`;
                        return (
                        <div
                          key={key}
                          className="tree-node is-clickable"
                          onClick={() => onSelectCacheSound?.(key)}
                          title={`${s.kind} · ${s.source}`}
                        >
                          <span className="tree-toggle">▶</span>
                          <span className="tree-icon">
                            {s.kind === "stream"
                              ? "≋"
                              : s.kind === "stream-missing"
                                ? "?"
                                : s.kind === "raw"
                                  ? "·"
                                  : "♪"}
                          </span>
                          <span className="tree-label">{s.name}</span>
                          <span className="tree-count dim mono">#{s.index}</span>
                        </div>
                        );
                      })}
                      {filtered.length > 500 && (
                        <div className="tree-empty small dim">
                          + {filtered.length - 500} more — narrow with the filter
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })()}
          </>
        )}

      </div>
    </div>
  );
}

/** Recursive renderer for the Asset Library tree. Folders collapse
 *  independently via the `collapsed` map; leaves are clickable rows
 *  that select the first placed instance of that asset (so the camera
 *  + Inspector update). When the user typed a filter, only matching
 *  leaves render — folders auto-expand to show matches.
 */
function AssetLibraryTree({
  node,
  depth,
  collapsed,
  setCollapsed,
  filter,
  instances,
  selection,
  onPreviewRawAsset,
  expandedAssets,
  setExpandedAssets,
  cacheTextureIds,
}: {
  node: AssetTreeNode;
  depth: number;
  collapsed: Record<string, boolean>;
  setCollapsed: React.Dispatch<React.SetStateAction<Record<string, boolean>>>;
  filter: string;
  instances: Instance[];
  selection: Selection;
  onPreviewRawAsset?: (assetTuid: string) => void;
  expandedAssets: Set<string>;
  setExpandedAssets: React.Dispatch<React.SetStateAction<Set<string>>>;
  cacheTextureIds: Set<number>;
}) {
  // Every folder defaults to collapsed at every depth — opening a
  // parent does NOT auto-expand its children. The user explicitly
  // expands the path they care about; with hundreds of entries deep
  // on bayou, auto-expanding inner folders dumped a wall of noise.
  // The `collapsed` map records explicit user toggles, which override
  // this default.
  const isRoot = depth === 0;
  const isCollapsed = collapsed[node.path] ?? true;

  const toggle = () =>
    setCollapsed((prev) => ({
      ...prev,
      [node.path]: !(prev[node.path] ?? true),
    }));

  // For leaves, find the first placed instance with this asset_tuid so
  // clicking actually selects something the camera can focus on. If
  // there's no placement (asset is in lookup but not placed in the
  
  const firstPlaced = node.asset
    ? instances.find(
        (i) => i.kind === node.kind && i.asset_tuid === node.asset!.asset_tuid,
      )
    : null;

  
  
  
  const matchesFilter = (n: AssetTreeNode): boolean => {
    if (!filter) return true;
    if (n.asset && n.label.toLowerCase().includes(filter)) return true;
    return n.children.some(matchesFilter);
  };
  if (!matchesFilter(node)) return null;

  
  
  const renderChildrenExpanded = filter ? true : !isCollapsed;

  if (node.asset) {
    const asset = node.asset;
    const isSelected = firstPlaced && selection.isSelected(firstPlaced.tuid);
    const isPrimary =
      firstPlaced && selection.primary === firstPlaced.tuid;
    const hasSubmeshes =
      (node.kind === "moby" || node.kind === "tie") &&
      Array.isArray(asset.submeshes) &&
      asset.submeshes.length > 0;
    const isAssetExpanded = expandedAssets.has(asset.asset_tuid);
    const toggleAsset = (e: React.MouseEvent) => {
      e.stopPropagation();
      setExpandedAssets((prev) => {
        const next = new Set(prev);
        if (next.has(asset.asset_tuid)) next.delete(asset.asset_tuid);
        else next.add(asset.asset_tuid);
        return next;
      });
    };
    return (
      <>
        <div
          className={`tree-node ${isSelected ? "selected" : ""} ${isPrimary ? "primary" : ""}`}
          style={{ paddingLeft: `${depth * 12}px` }}
          onClick={(e) => {
            e.stopPropagation();
            const mods = clickMods(e);
            if (mods.ctrl && firstPlaced) {
              selection.select(firstPlaced, mods);
            } else if (onPreviewRawAsset) {
              onPreviewRawAsset(asset.asset_tuid);
            } else if (firstPlaced) {
              selection.select(firstPlaced, mods);
            }
          }}
          title={
            firstPlaced
              ? `${asset.asset_tuid}\nClick: open preview · Ctrl-click: select world placement`
              : `${asset.asset_tuid}\nClick: open preview (asset is not placed in this level)`
          }
        >
          {hasSubmeshes ? (
            <span
              className="tree-toggle clickable"
              onClick={toggleAsset}
              title={isAssetExpanded ? "Hide submeshes" : "Show submeshes"}
            >
              {isAssetExpanded ? "▾" : "▸"}
            </span>
          ) : (
            <span className="tree-toggle" />
          )}
          <span className={`tree-icon kind-${node.kind}`}>
            {KIND_GLYPHS[node.kind] ?? "?"}
          </span>
          <span className="tree-label small">{node.label}</span>
          {!firstPlaced && (
            <span className="tree-count mono small dim" title="Asset is in lookup but not placed in the world">
              ∅
            </span>
          )}
        </div>
        {hasSubmeshes && isAssetExpanded && (
          <SubmeshList
            submeshes={asset.submeshes}
            cacheTextureIds={cacheTextureIds}
            depth={depth}
          />
        )}
      </>
    );
  }

  
  const leafCount = (function count(n: AssetTreeNode): number {
    if (n.asset) return 1;
    return n.children.reduce((acc, c) => acc + count(c), 0);
  })(node);

  return (
    <div className="hierarchy-library">
      <div
        className="tree-node"
        style={{ paddingLeft: isRoot ? 0 : `${depth * 12}px` }}
        onClick={toggle}
      >
        <span className="tree-toggle">
          {renderChildrenExpanded ? "▾" : "▸"}
        </span>
        <span className={`tree-icon kind-${node.kind}`}>
          {isRoot ? KIND_GLYPHS[node.kind] : "▦"}
        </span>
        <span className="tree-label">{node.label}</span>
        <span className="tree-count">{leafCount.toLocaleString()}</span>
      </div>
      {renderChildrenExpanded && (
        <div className="tree-children">
          {node.children.map((c) => (
            <AssetLibraryTree
              key={c.path}
              node={c}
              depth={depth + 1}
              collapsed={collapsed}
              setCollapsed={setCollapsed}
              filter={filter}
              instances={instances}
              selection={selection}
              onPreviewRawAsset={onPreviewRawAsset}
              expandedAssets={expandedAssets}
              setExpandedAssets={setExpandedAssets}
              cacheTextureIds={cacheTextureIds}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Inline submesh + texture list shown under each expanded asset leaf.
 * One row per submesh (indented), with three texture-id chips
 * (a/n/e). Each chip is rendered with a small color square — green if
 * the texture is present in the cache, purple (#bb55ff, same as
 * AssetWorkbench's missing-data tint) if the ID doesn't resolve.
 */
function SubmeshList({
  submeshes,
  cacheTextureIds,
  depth,
}: {
  submeshes: AssetMeshes["submeshes"];
  cacheTextureIds: Set<number>;
  depth: number;
}) {
  return (
    <div className="hierarchy-submesh-list">
      {submeshes.map((s, i) => (
        <div
          key={i}
          className="hierarchy-submesh-row"
          style={{ paddingLeft: `${(depth + 1) * 12 + 14}px` }}
          title={`Submesh ${i}`}
        >
          <span className="hierarchy-submesh-idx mono small dim">
            [{i}]
          </span>
          <span className="hierarchy-submesh-chips">
            <TextureChip slot="a" id={s.albedo_id} present={cacheTextureIds} />
            <TextureChip slot="n" id={s.normal_id} present={cacheTextureIds} />
            <TextureChip slot="e" id={s.emissive_id} present={cacheTextureIds} />
          </span>
        </div>
      ))}
    </div>
  );
}

function TextureChip({
  slot,
  id,
  present,
}: {
  slot: "a" | "n" | "e";
  id: number | null;
  present: Set<number>;
}) {
  if (id == null) {
    return (
      <span
        className="hierarchy-tex-chip empty"
        title={`No ${slot === "a" ? "albedo" : slot === "n" ? "normal" : "emissive"} bound`}
      >
        <span className="hierarchy-tex-chip-slot">{slot}</span>
        <span className="dim">—</span>
      </span>
    );
  }
  const isMissing = !present.has(id);
  return (
    <span
      className={`hierarchy-tex-chip ${isMissing ? "missing" : "ok"}`}
      title={`${slot === "a" ? "Albedo" : slot === "n" ? "Normal" : "Emissive"}: 0x${id.toString(16).toUpperCase().padStart(8, "0")}${
        isMissing ? " (not in cache — engine fallback)" : ""
      }`}
    >
      <span className="hierarchy-tex-chip-marker" />
      <span className="hierarchy-tex-chip-slot">{slot}</span>
      <span className="mono small">{id.toString(16).slice(-4)}</span>
    </span>
  );
}
