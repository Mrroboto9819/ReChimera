import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  addTabToPanel,
  closeTab,
  moveTab,
  setActiveTab,
  useAppDispatch,
  useAppSelector,
  type PanelId,
  type ViewId,
} from "../store";
import { ALL_VIEW_IDS, VIEW_META } from "../viewMeta";

const DRAG_MIME = "application/x-rechimera-tab";
const DRAG_ATTR = "data-tab-drag";

interface TabContainerProps {
  panelId: PanelId;
  views: Partial<Record<ViewId, ReactNode>>;
  className?: string;
}

interface DragPayload {
  viewId: ViewId;
  from: PanelId;
}

function encode(p: DragPayload): string {
  return `${p.from}:${p.viewId}`;
}
function decode(s: string): DragPayload | null {
  const [from, viewId] = s.split(":") as [PanelId, ViewId];
  if (!from || !viewId) return null;
  return { from, viewId };
}

export function TabContainer({ panelId, views, className }: TabContainerProps) {
  const { t } = useTranslation();
  const dispatch = useAppDispatch();
  const layout = useAppSelector((s) => s.panels.panels[panelId]);
  const allPanels = useAppSelector((s) => s.panels.panels);
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const pickerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!pickerOpen) return;
    const onDocClick = (e: MouseEvent) => {
      if (
        pickerRef.current &&
        e.target instanceof Node &&
        !pickerRef.current.contains(e.target)
      ) {
        setPickerOpen(false);
      }
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [pickerOpen]);

  const availableViews = useMemo(() => {
    if (!layout) return [];
    return ALL_VIEW_IDS.filter((vid) => {
      if (layout.tabs.includes(vid)) return false;
      const meta = VIEW_META[vid];
      if (!meta) return false;
      if (meta.singleton) {
        for (const id of Object.keys(allPanels) as PanelId[]) {
          if (id === panelId) continue;
          if (allPanels[id]?.tabs.includes(vid)) return false;
        }
      }
      return true;
    });
  }, [layout, allPanels, panelId]);

  if (!layout) return null;

  const activeId = layout.activeTab ?? layout.tabs[0] ?? null;
  const isEmpty = layout.tabs.length === 0;

  const onTabDragStart = (e: React.DragEvent, viewId: ViewId) => {
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData(DRAG_MIME, encode({ viewId, from: panelId }));
    e.dataTransfer.setData("text/plain", viewId);
    document.documentElement.setAttribute(DRAG_ATTR, "active");
  };

  const onTabDragEnd = () => {
    document.documentElement.removeAttribute(DRAG_ATTR);
    setDragOverIndex(null);
  };

  const onTabDragOver = (e: React.DragEvent, index: number) => {
    if (!e.dataTransfer.types.includes(DRAG_MIME)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    setDragOverIndex(index);
  };

  const onPanelDragOver = (e: React.DragEvent) => {
    if (!e.dataTransfer.types.includes(DRAG_MIME)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  };

  const onPanelDragLeave = (e: React.DragEvent) => {
    if (e.currentTarget === e.target) {
      setDragOverIndex(null);
    }
  };

  const performDrop = (e: React.DragEvent, insertIndex?: number) => {
    if (!e.dataTransfer.types.includes(DRAG_MIME)) return;
    e.preventDefault();
    const raw = e.dataTransfer.getData(DRAG_MIME);
    const payload = decode(raw);
    setDragOverIndex(null);
    document.documentElement.removeAttribute(DRAG_ATTR);
    if (!payload) return;
    if (payload.from === panelId && insertIndex == null) {
      dispatch(setActiveTab({ panelId, viewId: payload.viewId }));
      return;
    }
    dispatch(
      moveTab({
        viewId: payload.viewId,
        from: payload.from,
        to: panelId,
        insertIndex,
      }),
    );
  };

  return (
    <div
      className={`tab-container ${className ?? ""}`}
      onDragOver={onPanelDragOver}
      onDragLeave={onPanelDragLeave}
      onDrop={(e) => performDrop(e)}
    >
      <div className="tab-strip" role="tablist">
        {layout.tabs.map((tabId, i) => {
          const meta = VIEW_META[tabId];
          if (!meta) return null;
          const label = meta.i18nKey ? t(meta.i18nKey) : meta.label;
          const isActive = tabId === activeId;
          return (
            <div
              key={tabId}
              className={`tab-shell ${isActive ? "active" : ""} ${
                dragOverIndex === i ? "drop-here" : ""
              }`}
              draggable
              onDragStart={(e) => onTabDragStart(e, tabId)}
              onDragEnd={onTabDragEnd}
              onDragOver={(e) => onTabDragOver(e, i)}
              onDrop={(e) => {
                e.stopPropagation();
                performDrop(e, i);
              }}
            >
              <button
                role="tab"
                aria-selected={isActive}
                className="tab"
                onClick={() =>
                  dispatch(setActiveTab({ panelId, viewId: tabId }))
                }
                title={label}
              >
                {label}
              </button>
              <button
                type="button"
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  dispatch(closeTab({ panelId, viewId: tabId }));
                }}
                title="Close tab"
                aria-label={`Close ${label}`}
              >
                <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
                  <path
                    d="M2 2 L8 8 M8 2 L2 8"
                    stroke="currentColor"
                    strokeWidth="1.4"
                    strokeLinecap="round"
                  />
                </svg>
              </button>
            </div>
          );
        })}
        <div
          className="tab-strip-spacer"
          onDragOver={(e) => onTabDragOver(e, layout.tabs.length)}
          onDrop={(e) => {
            e.stopPropagation();
            performDrop(e, layout.tabs.length);
          }}
        />
        <div className="tab-add-wrap" ref={pickerRef}>
          <button
            type="button"
            className="tab-add-btn"
            onClick={() => setPickerOpen((v) => !v)}
            title={t("tabs.addView")}
            aria-label="Add view"
            disabled={availableViews.length === 0}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden>
              <path
                d="M6 2 V10 M2 6 H10"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
          {pickerOpen && availableViews.length > 0 && (
            <div className="tab-add-menu" role="menu">
              {availableViews.map((vid) => {
                const meta = VIEW_META[vid];
                if (!meta) return null;
                const label = meta.i18nKey ? t(meta.i18nKey) : meta.label;
                return (
                  <button
                    key={vid}
                    role="menuitem"
                    className="tab-add-menu-item"
                    onClick={() => {
                      dispatch(addTabToPanel({ panelId, viewId: vid }));
                      setPickerOpen(false);
                    }}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>
      <div className="tab-content">
        {isEmpty ? (
          <div className="tab-panel-empty">
            <svg
              width="42"
              height="42"
              viewBox="0 0 42 42"
              fill="none"
              aria-hidden
            >
              <rect
                x="3"
                y="9"
                width="36"
                height="27"
                rx="3"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeDasharray="3 3"
              />
              <path
                d="M21 17 V27 M16 22 H26"
                stroke="currentColor"
                strokeWidth="1.4"
                strokeLinecap="round"
              />
            </svg>
            <div className="tab-panel-empty-title">{t("tabs.emptyTitle")}</div>
            <div className="tab-panel-empty-hint small dim">
              {t("tabs.emptyHint")}
            </div>
          </div>
        ) : (
          activeId && (
            <div
              key={activeId}
              className="tab-panel active"
              role="tabpanel"
            >
              {views[activeId] ?? null}
            </div>
          )
        )}
      </div>
      <div className="tab-drop-zone" aria-hidden>
        <div className="tab-drop-zone-label">{t("tabs.dropMove")}</div>
      </div>
    </div>
  );
}
