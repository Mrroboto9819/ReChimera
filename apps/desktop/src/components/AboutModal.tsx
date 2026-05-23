import iconUrl from "../../icon.png?url";
import { Modal } from "./Modal";
import { APP_VERSION, APP_BRAND_NAME, APP_REPO_URL, APP_ISSUES_URL, openExternal } from "../version";










function onAboutClick(e: React.MouseEvent<HTMLDivElement>) {
  const target = e.target as HTMLElement | null;
  const anchor = target?.closest?.("a");
  if (anchor instanceof HTMLAnchorElement && anchor.href) {
    e.preventDefault();
    void openExternal(anchor.href);
  }
}

interface AboutModalProps {
  open: boolean;
  onClose: () => void;
}

interface CreditEntry {
  handle: string;
  githubUrl?: string;
  contribution: string;
}

const PEOPLE: CreditEntry[] = [
  {
    handle: "VELD-Dev",
    githubUrl: "https://github.com/VELD-Dev",
    contribution:
      "Author of ReLunacy — the C# / Unity predecessor that ReChimera ports its parser and rendering approach from. Lead maintainer of this project.",
  },
  {
    handle: "NefariousTechSupport",
    githubUrl: "https://github.com/NefariousTechSupport",
    contribution:
      "Original developer of Lunacy and key reverse engineer for the PS3-era Insomniac titles.",
  },
  {
    handle: "PredatorCZ",
    githubUrl: "https://github.com/PredatorCZ",
    contribution:
      "Pioneer of Ratchet & Clank: Future-series reverse engineering and author of InsomniaToolset + the Spike framework. Most of our SCREAM / IGHW / pointer-resolution rules come from cross-referencing their headers.",
  },
  {
    handle: "Nooga",
    contribution:
      "Artist behind ReLunacy's logo, which set the visual identity that this project's branding follows.",
  },
];










export function AboutModal({ open, onClose }: AboutModalProps) {
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={`About ReChimera v${APP_VERSION}`}
      subtitle="Offline level inspector and asset extractor for Insomniac Games' PS3 titles"
      size="lg"
      footer={
        <>
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => void openExternal(APP_REPO_URL)}
          >
            View on GitHub
          </button>
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => void openExternal(APP_ISSUES_URL)}
          >
            Report an issue
          </button>
          <button type="button" className="btn btn-primary" onClick={onClose}>
            Close
          </button>
        </>
      }
    >
      <div className="about-modal" onClick={onAboutClick}>
        <header className="about-header">
          <img src={iconUrl} alt="ReChimera" className="about-logo" />
          <div className="about-header-text">
            <div className="about-name">{APP_BRAND_NAME}</div>
            <div className="about-version mono small">
              v{APP_VERSION} · Beta
            </div>
            <div className="about-tagline small dim">
              Resistance: Fall of Man · Resistance 2 · Resistance 3 ·
              Ratchet &amp; Clank Future trilogy
            </div>
          </div>
        </header>

        <section className="about-section">
          <h3 className="about-section-title">Credits</h3>
          <p className="small dim">
            ReChimera stands on the shoulders of years of community
            reverse-engineering on Insomniac's PS3 engine. Heartfelt thanks
            to:
          </p>
          <ul className="about-people">
            {PEOPLE.map((p) => (
              <li key={p.handle} className="about-person">
                {p.githubUrl ? (
                  <a
                    href={p.githubUrl}
                    target="_blank"
                    rel="noreferrer"
                    className="about-handle"
                  >
                    @{p.handle}
                  </a>
                ) : (
                  <span className="about-handle">@{p.handle}</span>
                )}
                <span className="about-contribution small">
                  {p.contribution}
                </span>
              </li>
            ))}
          </ul>
        </section>

        <section className="about-section">
          <h3 className="about-section-title">Reference projects</h3>
          <ul className="about-refs small">
            <li>
              <a
                href="https://github.com/VELD-Dev/ReLunacy"
                target="_blank"
                rel="noreferrer"
              >
                ReLunacy / LibLunacy
              </a>{" "}
              <span className="dim">— GPL-3.0, by @VELD-Dev</span>
            </li>
            <li>
              <a
                href="https://github.com/PredatorCZ/InsomniaToolset"
                target="_blank"
                rel="noreferrer"
              >
                InsomniaToolset
              </a>{" "}
              <span className="dim">— GPL-3.0, by @PredatorCZ</span>
            </li>
            <li>
              <a
                href="https://github.com/PredatorCZ/Spike"
                target="_blank"
                rel="noreferrer"
              >
                Spike framework
              </a>{" "}
              <span className="dim">— BSD-3-Clause</span>
            </li>
          </ul>
        </section>

        <section className="about-section">
          <h3 className="about-section-title">License</h3>
          <p className="small dim">
            Distributed under{" "}
            <strong>GPL-3.0-or-later</strong>. See the LICENSE and
            NOTICE.md files in the repository for the full text and
            third-party attributions. Game data is not included; you
            must supply your own legitimately-acquired files.
          </p>
        </section>
      </div>
    </Modal>
  );
}
