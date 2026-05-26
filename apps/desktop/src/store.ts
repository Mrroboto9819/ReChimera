import {
  combineReducers,
  configureStore,
  createSlice,
  type PayloadAction,
} from "@reduxjs/toolkit";
import { useDispatch, useSelector, type TypedUseSelectorHook } from "react-redux";
import {
  FLUSH,
  PAUSE,
  PERSIST,
  PURGE,
  REGISTER,
  REHYDRATE,
  persistReducer,
  persistStore,
} from "redux-persist";
import storage from "redux-persist/lib/storage";






export interface ViewSettingsState {
  showMobys: boolean;
  showTies: boolean;
  showDetails: boolean;
  showShrubs: boolean;
  showFoliage: boolean;
  showLights: boolean;
  showEnvSamplers: boolean;
  showUFrags: boolean;
  showUFragBounds: boolean;
  showGrid: boolean;
  showAxes: boolean;

  showStats: boolean;



  showBones: boolean;


  playAnimation: boolean;

  showCollision: boolean;

  showSkyDome: boolean;

  skyboxTextureId: number | null;
}

const DEFAULT_VIEW: ViewSettingsState = {
  showMobys: true,
  showTies: true,
  showDetails: true,
  showShrubs: true,
  showFoliage: true,
  showLights: true,
  showEnvSamplers: false,
  showUFrags: true,
  showUFragBounds: false,
  showGrid: true,
  showAxes: true,
  showStats: false,
  showBones: false,
  playAnimation: true,
  showCollision: false,
  showSkyDome: true,
  skyboxTextureId: null,
};

export type BooleanViewKey = {
  [K in keyof ViewSettingsState]: ViewSettingsState[K] extends boolean
    ? K
    : never;
}[keyof ViewSettingsState];

const viewSlice = createSlice({
  name: "view",
  initialState: DEFAULT_VIEW,
  reducers: {
    toggleView(state, action: PayloadAction<BooleanViewKey>) {
      state[action.payload] = !state[action.payload];
    },
    setSkybox(state, action: PayloadAction<number | null>) {
      state.skyboxTextureId = action.payload;
    },
    resetView() {
      return DEFAULT_VIEW;
    },
  },
});







export interface LayoutState {
  hierarchyPct: number;
  inspectorPct: number;
  bottomPct: number;
  consoleCollapsed: boolean;
  hierarchyHidden: boolean;
  inspectorHidden: boolean;
}

const DEFAULT_LAYOUT: LayoutState = {
  hierarchyPct: 18,
  inspectorPct: 22,
  bottomPct: 24,
  consoleCollapsed: true,
  hierarchyHidden: false,
  inspectorHidden: false,
};

const layoutSlice = createSlice({
  name: "layout",
  initialState: DEFAULT_LAYOUT,
  reducers: {
    setHierarchyPct(state, action: PayloadAction<number>) {
      state.hierarchyPct = action.payload;
    },
    setInspectorPct(state, action: PayloadAction<number>) {
      state.inspectorPct = action.payload;
    },
    setBottomPct(state, action: PayloadAction<number>) {
      state.bottomPct = action.payload;
    },
    toggleConsoleCollapsed(state) {
      state.consoleCollapsed = !state.consoleCollapsed;
    },
    toggleHierarchyHidden(state) {
      state.hierarchyHidden = !state.hierarchyHidden;
    },
    toggleInspectorHidden(state) {
      state.inspectorHidden = !state.inspectorHidden;
    },
    resetLayout() {
      return DEFAULT_LAYOUT;
    },
  },
});

export const { toggleView, resetView, setSkybox } = viewSlice.actions;
export const {
  setHierarchyPct,
  setInspectorPct,
  setBottomPct,
  toggleConsoleCollapsed,
  toggleHierarchyHidden,
  toggleInspectorHidden,
  resetLayout,
} = layoutSlice.actions;

export type PanelId = "left" | "right" | "bottom" | "center";
export type ViewId = "hierarchy" | "inspector" | "console" | "viewport";

export interface PanelLayout {
  tabs: ViewId[];
  activeTab: ViewId | null;
}

export interface PanelsState {
  panels: Record<PanelId, PanelLayout>;
}

const DEFAULT_PANELS: PanelsState = {
  panels: {
    left: { tabs: ["hierarchy"], activeTab: "hierarchy" },
    right: { tabs: ["inspector"], activeTab: "inspector" },
    bottom: { tabs: ["console"], activeTab: "console" },
    center: { tabs: ["viewport"], activeTab: "viewport" },
  },
};

const panelsSlice = createSlice({
  name: "panels",
  initialState: DEFAULT_PANELS,
  reducers: {
    setActiveTab(
      state,
      action: PayloadAction<{ panelId: PanelId; viewId: ViewId }>,
    ) {
      const p = state.panels[action.payload.panelId];
      if (p && p.tabs.includes(action.payload.viewId)) {
        p.activeTab = action.payload.viewId;
      }
    },
    moveTab(
      state,
      action: PayloadAction<{
        viewId: ViewId;
        from: PanelId;
        to: PanelId;
        insertIndex?: number;
      }>,
    ) {
      const { viewId, from, to, insertIndex } = action.payload;
      const src = state.panels[from];
      const dst = state.panels[to];
      if (!src || !dst) return;
      const i = src.tabs.indexOf(viewId);
      if (i < 0) return;
      src.tabs.splice(i, 1);
      if (src.activeTab === viewId) {
        src.activeTab = src.tabs[0] ?? null;
      }
      const at =
        insertIndex == null
          ? dst.tabs.length
          : Math.max(0, Math.min(insertIndex, dst.tabs.length));
      const existingAt = dst.tabs.indexOf(viewId);
      if (existingAt >= 0) {
        dst.tabs.splice(existingAt, 1);
      }
      const finalAt = Math.min(at, dst.tabs.length);
      dst.tabs.splice(finalAt, 0, viewId);
      dst.activeTab = viewId;
    },
    addTabToPanel(
      state,
      action: PayloadAction<{ panelId: PanelId; viewId: ViewId }>,
    ) {
      const { panelId, viewId } = action.payload;
      const p = state.panels[panelId];
      if (!p) return;
      if (!p.tabs.includes(viewId)) {
        p.tabs.push(viewId);
      }
      p.activeTab = viewId;
    },
    closeTab(
      state,
      action: PayloadAction<{ panelId: PanelId; viewId: ViewId }>,
    ) {
      const { panelId, viewId } = action.payload;
      const p = state.panels[panelId];
      if (!p) return;
      const i = p.tabs.indexOf(viewId);
      if (i < 0) return;
      p.tabs.splice(i, 1);
      if (p.activeTab === viewId) {
        p.activeTab = p.tabs[0] ?? null;
      }
    },
    resetPanels() {
      return DEFAULT_PANELS;
    },
  },
});

export const {
  setActiveTab,
  moveTab,
  addTabToPanel,
  closeTab,
  resetPanels,
} = panelsSlice.actions;








export type ThemeMode = "dark" | "light";

/** A skin is an app-wide visual theme. Anyone can register a new one by
 *  pushing into `APP_SKINS` and shipping a CSS block scoped to
 *  `:root[data-app-skin="<id>"]`. The runtime applies the attribute on
 *  the document root; CSS does the rest. */
export type AppSkin = string;

export interface AppSkinDef {
  /** Stable id used as the `data-app-skin` attribute value. */
  id: AppSkin;
  /** Display label shown in Settings. */
  label: string;
  /** Color modes this skin supports. Dark-only skins (e.g. Resistance 2)
   *  list just ["dark"]; the runtime auto-snaps mode to the first allowed
   *  entry on switch. */
  allowedModes: ThemeMode[];
  /** Optional class added to `.modal-dialog` when this skin is active.
   *  Skins that fit inside the default modal shell can leave this empty. */
  modalClass?: string;
  /** One-line description shown under the skin label in Settings. */
  description?: string;
  /** When set, the runtime forces this color as the brand/primary while
   *  this skin is active, overriding the user's brand-color setting.
   *  Use for branded skins where the palette is part of the identity
   *  (e.g. Resistance 2's amber). */
  primaryColor?: string;
}

export const APP_SKINS: AppSkinDef[] = [
  {
    id: "default",
    label: "Default",
    allowedModes: ["dark", "light"],
    description: "The standard ReChimera look — light and dark variants.",
  },
  {
    id: "resistance2",
    label: "Resistance 2",
    allowedModes: ["dark"],
    modalClass: "modal-dialog--hud",
    description: "Amber HUD inspired by the Resistance 2 pause menu. Dark only.",
    primaryColor: "#d4b860",
  },
  {
    id: "resistance3",
    label: "Resistance 3",
    allowedModes: ["light"],
    modalClass: "modal-dialog--journal",
    description: "Aged-paper wartime journal — sepia, charcoal ink, deep red accents.",
    primaryColor: "#8a1f1f",
  },
  {
    id: "resistance1",
    label: "Resistance: Fall of Man",
    allowedModes: ["dark"],
    modalClass: "modal-dialog--metal",
    description: "Wartime industrial — rusted metal frame, olive panels, cream-yellow text.",
    primaryColor: "#dcc878",
  },
  {
    id: "racit",
    label: "R&C: A Crack in Time",
    allowedModes: ["dark"],
    modalClass: "modal-dialog--cosmic",
    description: "Cosmic navy with warm orange highlights — clean sci-fi inspired look.",
    primaryColor: "#f0a830",
  },
];

export function getAppSkin(id: AppSkin): AppSkinDef {
  return APP_SKINS.find((s) => s.id === id) ?? APP_SKINS[0]!;
}

export type Language = "en" | "es" | "fr" | "zh" | "ru";

export interface AssetColors {

  moby: string;

  tie: string;

  detail: string;

  shrub: string;

  foliage: string;

  light: string;

  envsampler: string;

  sky: string;

  ufrag: string;

  selection: string;


  proxy: string;
}

export interface SettingsState {
  theme: ThemeMode;
  appSkin: AppSkin;
  brandColor: string;
  assetColors: AssetColors;
  language: Language;
  uiSoundEnabled: boolean;
  uiSoundVolume: number;
}

const DEFAULT_SETTINGS: SettingsState = {
  theme: "dark",
  appSkin: "default",
  brandColor: "#FF6363",
  uiSoundEnabled: true,
  uiSoundVolume: 0.6,
  assetColors: {
    moby: "#ff8a3d",
    tie: "#3dd0ff",
    detail: "#b48cff",
    shrub: "#6bd47b",
    foliage: "#9be572",
    light: "#ffd83d",
    envsampler: "#46e0c8",
    sky: "#5d9bff",
    ufrag: "#97de82",
    selection: "#3eb1ff",
    proxy: "#8a8a8a",
  },
  language: "en",
};

const settingsSlice = createSlice({
  name: "settings",
  initialState: DEFAULT_SETTINGS,
  reducers: {
    setTheme(state, action: PayloadAction<ThemeMode>) {
      const allowed = getAppSkin(state.appSkin).allowedModes;
      state.theme = allowed.includes(action.payload) ? action.payload : (allowed[0] ?? "dark");
    },
    toggleTheme(state) {
      const allowed = getAppSkin(state.appSkin).allowedModes;
      if (allowed.length < 2) return;
      state.theme = state.theme === "dark" ? "light" : "dark";
    },
    setAppSkin(state, action: PayloadAction<AppSkin>) {
      state.appSkin = action.payload;
      const allowed = getAppSkin(action.payload).allowedModes;
      if (!allowed.includes(state.theme)) {
        state.theme = allowed[0] ?? "dark";
      }
    },
    setUiSoundEnabled(state, action: PayloadAction<boolean>) {
      state.uiSoundEnabled = action.payload;
    },
    setUiSoundVolume(state, action: PayloadAction<number>) {
      state.uiSoundVolume = Math.max(0, Math.min(1, action.payload));
    },
    setBrandColor(state, action: PayloadAction<string>) {
      state.brandColor = action.payload;
    },
    setAssetColor(
      state,
      action: PayloadAction<{ key: keyof AssetColors; value: string }>,
    ) {
      state.assetColors[action.payload.key] = action.payload.value;
    },
    setLanguage(state, action: PayloadAction<Language>) {
      state.language = action.payload;
    },
    resetSettings() {
      return DEFAULT_SETTINGS;
    },
  },
});

export const {
  setTheme,
  toggleTheme,
  setAppSkin,
  setBrandColor,
  setAssetColor,
  setLanguage,
  setUiSoundEnabled,
  setUiSoundVolume,
  resetSettings,
} = settingsSlice.actions;







const rootReducer = combineReducers({
  view: viewSlice.reducer,
  layout: layoutSlice.reducer,
  settings: settingsSlice.reducer,
  panels: panelsSlice.reducer,
});

const persistedReducer = persistReducer(
  {
    key: "rechimera-config",
    version: 5,
    storage,
    whitelist: ["view", "layout", "settings", "panels"],
  },
  rootReducer,
);

export const store = configureStore({
  reducer: persistedReducer,
  middleware: (gdm) =>
    gdm({
      serializableCheck: {
        ignoredActions: [FLUSH, REHYDRATE, PAUSE, PERSIST, PURGE, REGISTER],
      },
    }),
});

export const persistor = persistStore(store);

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;

export const useAppDispatch: () => AppDispatch = useDispatch;
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;


export function resetAll(dispatch: AppDispatch) {
  dispatch(resetView());
  dispatch(resetLayout());
  dispatch(resetSettings());
  dispatch(resetPanels());
}
