import { useEffect } from "react";
import i18n from "./i18n";
import { getAppSkin, useAppSelector } from "./store";


















export function useApplySettings(): void {
  const theme = useAppSelector((s) => s.settings.theme);
  const appSkin = useAppSelector((s) => s.settings.appSkin);
  const brand = useAppSelector((s) => s.settings.brandColor);
  const colors = useAppSelector((s) => s.settings.assetColors);
  const language = useAppSelector((s) => s.settings.language);

  useEffect(() => {
    const root = document.documentElement;
    root.setAttribute("data-theme", theme);
    root.style.colorScheme = theme;
  }, [theme]);

  useEffect(() => {
    document.documentElement.setAttribute("data-app-skin", appSkin);
  }, [appSkin]);

  useEffect(() => {
    if (i18n.language !== language) {
      void i18n.changeLanguage(language);
    }
    document.documentElement.setAttribute("lang", language);
  }, [language]);

  useEffect(() => {
    const root = document.documentElement;
    const skinPrimary = getAppSkin(appSkin).primaryColor;
    const effective = skinPrimary ?? brand;
    root.style.setProperty("--brand-color", effective);
    root.style.setProperty("--accent-blue", effective);
    if (/^#[0-9a-fA-F]{6}$/.test(effective)) {
      root.style.setProperty("--tint-blue", `${effective}26`);
    }
  }, [brand, appSkin]);

  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty("--asset-moby", colors.moby);
    root.style.setProperty("--asset-tie", colors.tie);
    root.style.setProperty("--asset-detail", colors.detail);
    root.style.setProperty("--asset-shrub", colors.shrub);
    root.style.setProperty("--asset-foliage", colors.foliage);
    root.style.setProperty("--asset-light", colors.light);
    root.style.setProperty("--asset-envsampler", colors.envsampler);
    root.style.setProperty("--asset-sky", colors.sky);
    root.style.setProperty("--asset-ufrag", colors.ufrag);
    root.style.setProperty("--asset-selection", colors.selection);
    root.style.setProperty("--asset-proxy", colors.proxy);
  }, [
    colors.moby,
    colors.tie,
    colors.detail,
    colors.light,
    colors.envsampler,
    colors.sky,
    colors.ufrag,
    colors.selection,
    colors.proxy,
  ]);
}







export function useAssetColors() {
  return useAppSelector((s) => s.settings.assetColors);
}
