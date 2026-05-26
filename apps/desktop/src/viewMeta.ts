import type { ViewId } from "./store";

export interface ViewMeta {
  id: ViewId;
  label: string;
  i18nKey?: string;
  singleton?: boolean;
}

export const VIEW_META: Record<ViewId, ViewMeta> = {
  hierarchy: { id: "hierarchy", label: "Hierarchy", i18nKey: "views.hierarchy" },
  inspector: { id: "inspector", label: "Inspector", i18nKey: "views.inspector" },
  console: { id: "console", label: "Console", i18nKey: "views.console" },
  viewport: {
    id: "viewport",
    label: "Viewport",
    i18nKey: "views.viewport",
    singleton: true,
  },
  assetWorkbench: {
    id: "assetWorkbench",
    label: "Asset",
    i18nKey: "views.assetWorkbench",
    singleton: true,
  },
};

export const ALL_VIEW_IDS: ViewId[] = [
  "hierarchy",
  "inspector",
  "console",
  "viewport",
  "assetWorkbench",
];
