import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import iconUrl from "../../icon.png?url";
import { Modal } from "./Modal";
import { SUPPORTED_LANGUAGES, type Language } from "../i18n";
import {
  APP_BRAND_NAME,
  APP_ISSUES_URL,
  APP_REPO_URL,
  APP_VERSION,
  openExternal,
} from "../version";
import {
  resetSettings,
  setAssetColor,
  setBrandColor,
  setLanguage,
  setTheme,
  useAppDispatch,
  useAppSelector,
  type AssetColors,
  type ThemeMode,
} from "../store";

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
}

type TabKey = "general" | "colors" | "about";

interface Credit {
  handle: string;
  githubUrl?: string;
  role: string;
}

const CREDITS: Credit[] = [
  {
    handle: "VELD-Dev",
    githubUrl: "https://github.com/VELD-Dev",
    role: "Author of ReLunacy. Lead maintainer of this project.",
  },
  {
    handle: "NefariousTechSupport",
    githubUrl: "https://github.com/NefariousTechSupport",
    role: "Original Lunacy + IGHW reverse-engineering work.",
  },
  {
    handle: "PredatorCZ",
    githubUrl: "https://github.com/PredatorCZ",
    role: "InsomniaToolset author. SCREAM / pointer-resolution rules.",
  },
  {
    handle: "Nooga",
    role: "Artist behind the logo + visual identity.",
  },
];

export function SettingsModal({ open, onClose }: SettingsModalProps) {
  const { t } = useTranslation();
  const dispatch = useAppDispatch();
  const theme = useAppSelector((s) => s.settings.theme);
  const brandColor = useAppSelector((s) => s.settings.brandColor);
  const assetColors = useAppSelector((s) => s.settings.assetColors);
  const language = useAppSelector((s) => s.settings.language);

  const [tab, setTab] = useState<TabKey>("general");

  const tabs: { key: TabKey; label: string }[] = [
    { key: "general", label: t("settings.tabGeneral") },
    { key: "colors", label: t("settings.tabColors") },
    { key: "about", label: t("settings.tabAbout") },
  ];

  const assetRows: { key: keyof AssetColors; label: string; hint: string }[] = [
    { key: "moby", label: t("settings.moby"), hint: t("settings.mobyHint") },
    { key: "tie", label: t("settings.tie"), hint: t("settings.tieHint") },
    {
      key: "detail",
      label: "Detail clusters",
      hint: "Static debris/props (RFOM only)",
    },
    {
      key: "shrub",
      label: "Shrubs",
      hint: "Vegetation meshes (RFOM only)",
    },
    {
      key: "foliage",
      label: "Foliage",
      hint: "Sprite foliage / grass (RFOM only)",
    },
    {
      key: "light",
      label: "Lights",
      hint: "Point/area light placements (RFOM only)",
    },
    {
      key: "envsampler",
      label: "Env probes",
      hint: "Cubemap reflection probes (RFOM only)",
    },
    {
      key: "sky",
      label: "Cubemap",
      hint: "Cubemap badge + viewport tint",
    },
    { key: "ufrag", label: t("settings.ufrag"), hint: t("settings.ufragHint") },
    {
      key: "selection",
      label: t("settings.selection"),
      hint: t("settings.selectionHint"),
    },
    { key: "proxy", label: t("settings.proxy"), hint: t("settings.proxyHint") },
  ];

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      bodyClassName="settings-modal-body"
      footer={
        <div
          style={{
            display: "flex",
            gap: 8,
            justifyContent: "space-between",
            width: "100%",
          }}
        >
          <button
            className="btn"
            onClick={() => {
              if (window.confirm(t("common.confirmReset"))) {
                dispatch(resetSettings());
              }
            }}
          >
            {t("common.reset")}
          </button>
          <button className="btn btn-primary" onClick={onClose}>
            {t("common.done")}
          </button>
        </div>
      }
    >
      <header className="settings-hero">
        <img src={iconUrl} alt="" className="settings-hero-logo" />
        <div className="settings-hero-text">
          <div className="settings-hero-name">{APP_BRAND_NAME}</div>
          <div className="settings-hero-meta small dim">
            v{APP_VERSION} · by VELD-Dev &amp; contributors
          </div>
        </div>
      </header>

      <nav className="settings-tabs" role="tablist">
        {tabs.map((tb) => (
          <button
            key={tb.key}
            role="tab"
            aria-selected={tab === tb.key}
            className={`settings-tab ${tab === tb.key ? "active" : ""}`}
            onClick={() => setTab(tb.key)}
          >
            {tb.label}
          </button>
        ))}
      </nav>

      <div className="settings-tab-content">
        {tab === "general" && (
          <>
            <div className="settings-ai-banner">
              <span className="settings-ai-banner-icon" aria-hidden>
                ✨
              </span>
              <div className="settings-ai-banner-text small">
                {t("ai.disclaimer")}{" "}
                <a
                  className="settings-ai-banner-link"
                  href={APP_REPO_URL}
                  onClick={(e) => {
                    e.preventDefault();
                    void openExternal(APP_REPO_URL);
                  }}
                >
                  {t("ai.submitPR")}
                </a>
              </div>
            </div>

            <section className="settings-section">
              <div className="settings-row">
                <div className="settings-row-text">
                  <div className="settings-row-label">{t("settings.language")}</div>
                  <div className="settings-row-hint small dim">
                    {t("settings.languageHint")}
                  </div>
                </div>
                <select
                  className="settings-language-select"
                  value={language}
                  onChange={(e) =>
                    dispatch(setLanguage(e.target.value as Language))
                  }
                >
                  {SUPPORTED_LANGUAGES.map((lang) => (
                    <option key={lang.code} value={lang.code}>
                      {lang.label}
                    </option>
                  ))}
                </select>
              </div>

              <div className="settings-row">
                <div className="settings-row-text">
                  <div className="settings-row-label">{t("settings.theme")}</div>
                  <div className="settings-row-hint small dim">
                    {t("settings.themeHint")}
                  </div>
                </div>
                <div className="settings-theme-toggle">
                  {(["dark", "light"] as ThemeMode[]).map((tm) => (
                    <button
                      key={tm}
                      className={`btn ${theme === tm ? "btn-primary" : ""}`}
                      onClick={() => dispatch(setTheme(tm))}
                    >
                      {tm === "dark"
                        ? t("settings.themeDark")
                        : t("settings.themeLight")}
                    </button>
                  ))}
                </div>
              </div>
            </section>
          </>
        )}

        {tab === "colors" && (
          <>
            <section className="settings-section">
              <h4 className="settings-heading">{t("settings.appearance")}</h4>
              <div className="settings-row">
                <div className="settings-row-text">
                  <div className="settings-row-label">
                    {t("settings.brandColor")}
                  </div>
                  <div className="settings-row-hint small dim">
                    {t("settings.brandColorHint")}
                  </div>
                </div>
                <ColorField
                  value={brandColor}
                  onChange={(v) => dispatch(setBrandColor(v))}
                />
              </div>
            </section>

            <section className="settings-section">
              <h4 className="settings-heading">{t("settings.assetColors")}</h4>
              <p
                className="small dim"
                style={{ margin: "0 0 12px", lineHeight: 1.5 }}
              >
                {t("settings.assetColorsHint")}
              </p>
              {assetRows.map((row) => (
                <div className="settings-row" key={row.key}>
                  <div className="settings-row-text">
                    <div className="settings-row-label">{row.label}</div>
                    <div className="settings-row-hint small dim">{row.hint}</div>
                  </div>
                  <ColorField
                    value={assetColors[row.key]}
                    onChange={(v) =>
                      dispatch(setAssetColor({ key: row.key, value: v }))
                    }
                  />
                </div>
              ))}
            </section>
          </>
        )}

        {tab === "about" && (
          <section className="settings-section">
            <p className="small" style={{ marginTop: 0, lineHeight: 1.6 }}>
              {t("settings.aboutBlurb")}
            </p>
            <h4 className="settings-heading">{t("settings.credits")}</h4>
            <ul className="settings-credits">
              {CREDITS.map((c) => (
                <li key={c.handle} className="settings-credit-row">
                  {c.githubUrl ? (
                    <a
                      className="settings-credit-handle"
                      href={c.githubUrl}
                      onClick={(e) => {
                        e.preventDefault();
                        void openExternal(c.githubUrl!);
                      }}
                    >
                      @{c.handle}
                    </a>
                  ) : (
                    <span className="settings-credit-handle">@{c.handle}</span>
                  )}
                  <span className="small dim">{c.role}</span>
                </li>
              ))}
            </ul>
            <div className="settings-about-links">
              <button
                type="button"
                className="btn"
                onClick={() => void openExternal(APP_REPO_URL)}
              >
                {t("settings.viewOnGithub")}
              </button>
              <button
                type="button"
                className="btn"
                onClick={() => void openExternal(APP_ISSUES_URL)}
              >
                {t("settings.reportIssue")}
              </button>
            </div>
          </section>
        )}
      </div>
    </Modal>
  );
}

interface ColorFieldProps {
  value: string;
  onChange: (next: string) => void;
}

function ColorField({ value, onChange }: ColorFieldProps) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const tryCommit = (v: string) => {
    const trimmed = v.trim();
    if (/^#[0-9a-fA-F]{6}$/.test(trimmed)) onChange(trimmed);
  };
  return (
    <div className="settings-color-field">
      <input
        type="color"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="settings-color-swatch"
        aria-label="Pick color"
      />
      <input
        type="text"
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          tryCommit(e.target.value);
        }}
        onBlur={() => setDraft(value)}
        spellCheck={false}
        maxLength={7}
        className="settings-color-text mono"
        aria-label="Color hex"
      />
    </div>
  );
}
