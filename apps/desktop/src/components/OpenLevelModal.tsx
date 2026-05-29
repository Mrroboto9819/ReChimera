import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, Archive, File, Folder, Package, Lock, X } from "lucide-react";
import gsap from "gsap";
import { Modal } from "./Modal";
import {
  FallbackImage,
  franchisePlaceholder,
  levelThumbCandidatesFromPath,
} from "./FallbackImage";
import { Button } from "../ui";
import { useFileDrop } from "../useFileDrop";
import { psarcExtractStream } from "../api";
import { R2Wizard } from "./R2Wizard";

interface OpenLevelModalProps {
  open: boolean;
  busy: boolean;
  onClose: () => void;
  onOpen: (folderPath: string, opts?: { skipCachePrompt?: boolean; game?: string }) => void;
}

type GameId = "r1" | "r2" | "r3" | "rc_tod" | "rc_acit" | "rc_ffa" | "rc_a4o";
type Step = "game" | "source" | "psarc" | "folder";

interface PsarcExtractStatus {
  total: number;
  current: number;
  lastFile: string;
}

type Franchise = "resistance" | "ratchet_clank";

type CapabilityState = "ok" | "partial" | "missing" | "tpose";

interface Capabilities {
  meshes: CapabilityState;
  textures: CapabilityState;
  armatures: CapabilityState;
  animations: CapabilityState;
}

interface GameSpec {
  id: GameId;
  label: string;
  short: string;
  franchise: Franchise;
  supported: boolean;
  logoSrc: string;
  byline: string;
  entryFile: "assetlookup.dat" | "main.dat" | "ps3levelmain.dat";
  hint: string;
  capabilities: Capabilities;
}

const GAMES: GameSpec[] = [
  {
    id: "r1",
    label: "Resistance: Fall of Man",
    short: "RFOM",
    franchise: "resistance",
    supported: true,
    logoSrc: "/rfom.jpg",
    byline: "Full pipeline: meshes, textures, skeletons, animations, ufrags, ties, foliage, shrubs, details, cubemap, sounds, visemes.",
    entryFile: "ps3levelmain.dat",
    hint: "RFOM ships `game.psarc` archives — extract them first with PSARC tooling so you get the loose file tree (e.g. `extract_psarc.cmd` from the InsomniaToolset bundle, or any PSARC unpacker). Once unpacked, levels live in `<game>/PS3_GAME/USRDIR/packed/levels/<levelN>/`. Pick a folder that contains `ps3levelmain.dat` plus its siblings (`ps3leveltexs.dat`, `ps3levelverts.dat`, `ps3levelcoll.dat`, …). Note: some level folders (e.g. `level22`) only ship dialogue/sound and are NOT playable on their own.",
    capabilities: {
      meshes: "ok",
      textures: "ok",
      armatures: "ok",
      animations: "ok",
    },
  },
  {
    id: "r2",
    label: "Resistance 2",
    short: "R2",
    franchise: "resistance",
    supported: true,
    logoSrc: "/r2.jpg",
    byline: "Stable end-to-end: meshes, textures, materials, skeletons, animations, ufrags, sounds (SFX / Dialog / Music).",
    entryFile: "assetlookup.dat",
    hint: "R2 levels are V2 layout — assetlookup.dat plus mobys.dat / ties.dat / shaders.dat / textures.dat / highmips.dat / animsets.dat / zones.dat side-by-side.",
    capabilities: {
      meshes: "ok",
      textures: "ok",
      armatures: "ok",
      animations: "ok",
    },
  },
  {
    id: "r3",
    label: "Resistance 3",
    short: "R3",
    franchise: "resistance",
    supported: true,
    logoSrc: "/r3.jpg",
    byline: "Stable end-to-end: meshes, textures, materials, skeletons, animations, ufrags, sounds (SFX / Dialog / Music).",
    entryFile: "assetlookup.dat",
    hint: "R3 levels are V2 layout — same sibling .dat set as R2.",
    capabilities: {
      meshes: "ok",
      textures: "ok",
      armatures: "ok",
      animations: "ok",
    },
  },
  {
    id: "rc_tod",
    label: "Ratchet & Clank Future: Tools of Destruction",
    short: "R&C ToD",
    franchise: "ratchet_clank",
    supported: true,
    logoSrc: "/rc_tod.jpg",
    byline: "Meshes, textures, materials, skeletons, ufrags and tie instances load. Animations export in T-pose (complex frame format unsolved). No cubemap — no IT/ReLunacy reference.",
    entryFile: "main.dat",
    hint: "ToD levels are TOD layout — main.dat embeds asset tables, with vertices.dat / textures.dat / texstream.dat / system.tp / system.tph as siblings. There is no assetlookup.dat.",
    capabilities: {
      meshes: "ok",
      textures: "ok",
      armatures: "ok",
      animations: "tpose",
    },
  },
  {
    id: "rc_acit",
    label: "Ratchet & Clank Future: A Crack in Time",
    short: "R&C ACiT",
    franchise: "ratchet_clank",
    supported: true,
    logoSrc: "/rc_acit.jpg",
    byline: "Experimental — ACiT ships V2 layout (assetlookup.dat) so it routes through the R2/R3 parser. Skeletons read via the shift-recovery cascade; meshes / textures / ties may have ACiT-specific quirks still being investigated.",
    entryFile: "assetlookup.dat",
    hint: "A Crack in Time levels ship `assetlookup.dat` plus the standard V2 sibling set (mobys.dat / ties.dat / shaders.dat / textures.dat / highmips.dat / animsets.dat / zones.dat). Pick the folder that contains them. Some assets may not render correctly yet — please share the cache-build log if you hit issues.",
    capabilities: {
      meshes: "partial",
      textures: "partial",
      armatures: "partial",
      animations: "partial",
    },
  },
  {
    id: "rc_ffa",
    label: "Ratchet & Clank: Full Frontal Assault",
    short: "R&C FFA",
    franchise: "ratchet_clank",
    supported: false,
    logoSrc: "/R&C_FA.png",
    byline: "Not yet supported. V2 layout but not validated end-to-end yet — pending a test pass against real FFA level data.",
    entryFile: "assetlookup.dat",
    hint: "FFA levels are V2 layout — same sibling .dat set as R2 / R3. Once support is enabled this should route through the R2/R3 parser.",
    capabilities: {
      meshes: "missing",
      textures: "missing",
      armatures: "missing",
      animations: "missing",
    },
  },
  {
    id: "rc_a4o",
    label: "Ratchet & Clank: All 4 One",
    short: "R&C A4O",
    franchise: "ratchet_clank",
    supported: true,
    logoSrc: "/rc_a4.jpg",
    byline: "V2 layout. Meshes, textures, materials, skeletons, animations and sounds all working.",
    entryFile: "assetlookup.dat",
    hint: "All 4 One levels use V2 layout — assetlookup.dat plus mobys.dat / ties.dat / shaders.dat / textures.dat / highmips.dat / animsets.dat / zones.dat side-by-side. Same as R2/R3/FFA.",
    capabilities: {
      meshes: "ok",
      textures: "ok",
      armatures: "ok",
      animations: "ok",
    },
  },
];

const FRANCHISES: Array<{ id: Franchise; label: string }> = [
  { id: "resistance", label: "Resistance" },
  { id: "ratchet_clank", label: "Ratchet & Clank" },
];

const CAPABILITY_LABELS: Array<{ key: keyof Capabilities; label: string }> = [
  { key: "meshes", label: "Meshes" },
  { key: "textures", label: "Textures" },
  { key: "armatures", label: "Armatures" },
  { key: "animations", label: "Animations" },
];

function capTooltip(label: string, state: CapabilityState): string {
  switch (state) {
    case "ok":
      return `${label}: works`;
    case "partial":
      return `${label}: works, some assets may be missing`;
    case "missing":
      return `${label}: not yet supported`;
    case "tpose":
      return `${label}: T-pose only — frame decode unsolved`;
  }
}

function makeAcceptLevelDrop(entryFile: string) {
  return (p: string): boolean => {
    const lower = p.toLowerCase();
    if (lower.endsWith(entryFile.toLowerCase())) return true;
    return !/\.[a-z0-9]{1,6}$/i.test(p);
  };
}

function parentDir(p: string): string {
  const sep = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return sep > 0 ? p.slice(0, sep) : p;
}

const RECENT_KEY = "rechimera.recentLevelsByGame";
const RECENT_MAX = 6;

type RecentMap = Partial<Record<GameId, string[]>>;

function loadAllRecent(): RecentMap {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const out: RecentMap = {};
    for (const id of Object.keys(parsed) as GameId[]) {
      const list = (parsed as RecentMap)[id];
      if (Array.isArray(list)) {
        out[id] = list.filter((s): s is string => typeof s === "string");
      }
    }
    return out;
  } catch {
    return {};
  }
}

function saveAllRecent(map: RecentMap) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(map));
  } catch {
    /* noop */
  }
}

function loadRecent(game: GameId | null): string[] {
  if (!game) return [];
  return loadAllRecent()[game] ?? [];
}

function pushRecent(game: GameId, folder: string): string[] {
  const all = loadAllRecent();
  const current = (all[game] ?? []).filter((p) => p !== folder);
  const next = [folder, ...current].slice(0, RECENT_MAX);
  all[game] = next;
  saveAllRecent(all);
  return next;
}

function dropRecent(game: GameId, folder: string): string[] {
  const all = loadAllRecent();
  const next = (all[game] ?? []).filter((p) => p !== folder);
  all[game] = next;
  saveAllRecent(all);
  return next;
}

function lastTwoSegments(path: string): string {
  const norm = path.replace(/\\/g, "/").replace(/\/+$/, "");
  const parts = norm.split("/");
  return parts.slice(-2).join(" / ") || norm;
}


const FRANCHISE_TAB_KEY = "rechimera.wizardFranchiseTab";

function loadFranchiseTab(): Franchise {
  try {
    const v = localStorage.getItem(FRANCHISE_TAB_KEY);
    if (v === "resistance" || v === "ratchet_clank") return v;
  } catch {
    /* noop */
  }
  return "resistance";
}

function saveFranchiseTab(f: Franchise) {
  try {
    localStorage.setItem(FRANCHISE_TAB_KEY, f);
  } catch {
    /* noop */
  }
}

export function OpenLevelModal({
  open,
  busy,
  onClose,
  onOpen,
}: OpenLevelModalProps) {
  const [step, setStep] = useState<Step>("game");
  const [game, setGame] = useState<GameId | null>(null);
  const [r2WizardOpen, setR2WizardOpen] = useState(false);
  const [path, setPath] = useState("");
  const [warning, setWarning] = useState<string | null>(null);
  const [recent, setRecent] = useState<string[]>([]);
  const [franchiseTab, setFranchiseTab] = useState<Franchise>(loadFranchiseTab);

  const [psarcInput, setPsarcInput] = useState("");
  const [psarcOutput, setPsarcOutput] = useState("");
  const [psarcBusy, setPsarcBusy] = useState(false);
  const [psarcError, setPsarcError] = useState<string | null>(null);
  const [psarcProgress, setPsarcProgress] = useState<PsarcExtractStatus | null>(null);
  const [psarcDone, setPsarcDone] = useState(false);
  const psarcTotalRef = useRef(0);

  useEffect(() => {
    if (open) {
      setWarning(null);
      setGame(null);
      setRecent([]);
      setStep("game");
      setPsarcInput("");
      setPsarcOutput("");
      setPsarcBusy(false);
      setPsarcError(null);
      setPsarcProgress(null);
      setPsarcDone(false);
      setR2WizardOpen(false);
    }
  }, [open]);

  const pickFranchise = useCallback((f: Franchise) => {
    setFranchiseTab(f);
    saveFranchiseTab(f);
  }, []);

  const pickGame = useCallback((id: GameId) => {
    setGame(id);
    setRecent(loadRecent(id));
    setPath("");
    setWarning(null);
    setPsarcInput("");
    setPsarcOutput("");
    setPsarcError(null);
    setPsarcProgress(null);
    setPsarcDone(false);
    // The USRDIR wizard handles any Insomniac game whose USRDIR layout
    // matches `packed/game/` + `packed/levels/<level>/` — that's both V2
    // (R2/R3/ACiT/A4O/FFA, assetlookup.dat marker) and RFOM
    // (ps3levelmain.dat marker). The per-game ready filename is passed
    // down via the wizard's `entryFile` prop. ToD's `main.dat` layout
    // is different (no per-level psarc tree) so it still uses the
    // generic folder picker.
    const spec = GAMES.find((g) => g.id === id);
    const wizardEligible =
      spec?.supported &&
      (spec.entryFile === "assetlookup.dat" ||
        spec.entryFile === "ps3levelmain.dat");
    if (wizardEligible) {
      setR2WizardOpen(true);
      return;
    }
    setStep("source");
  }, []);

  const selectedGameSpec = game ? GAMES.find((g) => g.id === game) : null;
  const entryFile = selectedGameSpec?.entryFile ?? "assetlookup.dat";

  const handleBrowseFile = useCallback(async () => {
    setWarning(null);
    try {
      const picked = await openDialog({
        directory: false,
        multiple: false,
        title: `Pick ${entryFile}`,
        filters: [
          { name: "Insomniac level entry", extensions: ["dat"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
      if (typeof picked !== "string") return;
      const lastSep = Math.max(picked.lastIndexOf("/"), picked.lastIndexOf("\\"));
      const folder = lastSep > 0 ? picked.slice(0, lastSep) : picked;
      const filename = lastSep >= 0 ? picked.slice(lastSep + 1) : picked;
      setPath(folder);
      if (filename.toLowerCase() !== entryFile.toLowerCase()) {
        setWarning(
          `You picked "${filename}" — the parser will look for ${entryFile} in this folder.`,
        );
      }
    } catch (e) {
      setWarning(`File picker failed: ${e}`);
    }
  }, [entryFile]);

  const handleBrowseFolder = useCallback(async () => {
    setWarning(null);
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Pick a level folder",
      });
      if (typeof picked === "string") setPath(picked);
    } catch (e) {
      setWarning(`Folder picker failed: ${e}`);
    }
  }, []);

  const confirm = useCallback(
    (folder: string) => {
      const trimmed = folder.trim();
      if (!trimmed) return;
      if (game) {
        pushRecent(game, trimmed);
      }
      onOpen(trimmed, { game: GAMES.find((g) => g.id === game)?.short });
    },
    [onOpen, game],
  );

  const handleConfirm = useCallback(() => confirm(path), [confirm, path]);

  const handleDrop = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) {
        setWarning(`Drop a folder or a \`${entryFile}\` file.`);
        return;
      }
      const first = paths[0]!;
      const folder = first.toLowerCase().endsWith(entryFile.toLowerCase())
        ? parentDir(first)
        : first;
      setWarning(null);
      setPath(folder);
    },
    [entryFile],
  );

  const dropAccept = useMemo(() => makeAcceptLevelDrop(entryFile), [entryFile]);

  const dropPhase = useFileDrop({
    enabled: open && !busy && step === "folder",
    accept: dropAccept,
    onDrop: handleDrop,
  });

  const removeRecent = useCallback(
    (folder: string) => {
      if (!game) return;
      setRecent(dropRecent(game, folder));
    },
    [game],
  );

  const showPsarcStep = step === "psarc" || (step === "folder" && psarcDone);
  const stepIndex =
    step === "game"
      ? 1
      : step === "source"
        ? 2
        : step === "psarc"
          ? 3
          : showPsarcStep
            ? 4
            : 3;

  const stepRefs = useRef<Array<HTMLLIElement | null>>([null, null, null]);
  const prevStepIndexRef = useRef<number>(stepIndex);
  const stepbarMountedRef = useRef<boolean>(false);

  useEffect(() => {
    if (!open) {
      stepbarMountedRef.current = false;
      prevStepIndexRef.current = stepIndex;
      return;
    }
    const items = stepRefs.current.filter((el): el is HTMLLIElement => el !== null);
    if (items.length === 0) return;

    if (!stepbarMountedRef.current) {
      stepbarMountedRef.current = true;
      gsap.fromTo(
        items,
        { autoAlpha: 0, y: -8 },
        {
          autoAlpha: 1,
          y: 0,
          duration: 0.35,
          ease: "power3.out",
          stagger: 0.08,
        },
      );
      prevStepIndexRef.current = stepIndex;
      return;
    }

    const prev = prevStepIndexRef.current;
    if (prev === stepIndex) return;

    const activeEl = items[stepIndex - 1];
    if (activeEl) {
      const circle = activeEl.querySelector(".wizard-step-num");
      if (circle) {
        gsap.fromTo(
          circle,
          { scale: 0.85 },
          { scale: 1, duration: 0.45, ease: "elastic.out(1, 0.5)" },
        );
      }
    }

    if (stepIndex > prev) {
      const completedEl = items[prev - 1];
      if (completedEl) {
        const circle = completedEl.querySelector(".wizard-step-num");
        if (circle) {
          gsap.fromTo(
            circle,
            { scale: 1.25, boxShadow: "0 0 0 8px rgba(239, 68, 68, 0.35)" },
            {
              scale: 1,
              boxShadow: "0 0 0 0px rgba(239, 68, 68, 0)",
              duration: 0.55,
              ease: "power2.out",
            },
          );
        }
      }
    }

    prevStepIndexRef.current = stepIndex;
  }, [stepIndex, open]);
  const totalSteps = showPsarcStep ? 4 : 3;
  const subtitle =
    step === "game" ? (
      <span className="dim small">Step 1 / {totalSteps} · pick the game these files come from</span>
    ) : step === "source" ? (
      <span className="dim small">
        Step 2 / {totalSteps} ·{" "}
        <strong>{selectedGameSpec?.short}</strong> · choose where the data is
      </span>
    ) : step === "psarc" ? (
      <span className="dim small">
        Step 3 / 4 · <strong>{selectedGameSpec?.short}</strong> · extract a PSARC archive
      </span>
    ) : (
      <span className="dim small">
        Step {showPsarcStep ? 4 : 3} / {totalSteps} ·{" "}
        <strong>{selectedGameSpec?.short}</strong> · point to the level folder
      </span>
    );

  const handlePickPsarc = useCallback(() => {
    setPath("");
    setWarning(null);
    setPsarcInput("");
    setPsarcOutput("");
    setPsarcError(null);
    setPsarcProgress(null);
    setPsarcDone(false);
    setStep("psarc");
  }, []);

  const handleBrowsePsarcInput = useCallback(async () => {
    setPsarcError(null);
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
      if (typeof picked === "string") setPsarcInput(picked);
    } catch (e) {
      setPsarcError(`File picker failed: ${e}`);
    }
  }, []);

  const handleBrowsePsarcOutput = useCallback(async () => {
    setPsarcError(null);
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "Pick an output folder",
      });
      if (typeof picked === "string") setPsarcOutput(picked);
    } catch (e) {
      setPsarcError(`Folder picker failed: ${e}`);
    }
  }, []);

  const handleExtractPsarc = useCallback(async () => {
    if (!psarcInput.trim() || !psarcOutput.trim()) return;
    setPsarcBusy(true);
    setPsarcError(null);
    setPsarcDone(false);
    psarcTotalRef.current = 0;
    setPsarcProgress({ total: 0, current: 0, lastFile: "" });
    try {
      await psarcExtractStream(psarcInput.trim(), psarcOutput.trim(), (e) => {
        switch (e.type) {
          case "total":
            psarcTotalRef.current = e.total;
            setPsarcProgress({ total: e.total, current: 0, lastFile: "" });
            break;
          case "file":
            setPsarcProgress({
              total: psarcTotalRef.current,
              current: e.index,
              lastFile: e.name,
            });
            break;
          case "done":
            setPsarcProgress((p) =>
              p ? { ...p, current: p.total, lastFile: "Done." } : p,
            );
            setPsarcDone(true);
            setPsarcInput("");
            break;
          case "error":
            setPsarcError(e.message);
            break;
        }
      });
    } catch (e) {
      setPsarcError(String(e));
    } finally {
      setPsarcBusy(false);
    }
  }, [psarcInput, psarcOutput]);

  const handleContinueAfterExtract = useCallback(() => {
    if (psarcOutput.trim()) {
      setPath(psarcOutput.trim());
    }
    setStep("folder");
  }, [psarcOutput]);

  if (r2WizardOpen && game) {
    const wizardSpec = GAMES.find((g) => g.id === game);
    const wizardLabel = wizardSpec?.short ?? "R2";
    const wizardEntryFile = wizardSpec?.entryFile ?? "assetlookup.dat";
    return (
      <R2Wizard
        open={open}
        busy={busy}
        gameId={game}
        gameLabel={wizardLabel}
        entryFile={wizardEntryFile}
        onClose={() => {
          setR2WizardOpen(false);
          onClose();
        }}
        onOpen={(folder, opts) => {
          pushRecent(game, folder);
          onOpen(folder, { ...opts, game: wizardLabel });
        }}
      />
    );
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={
        step === "game"
          ? "Pick a game"
          : step === "source"
            ? "Choose source"
            : step === "psarc"
              ? "Extract PSARC"
              : "Open level"
      }
      subtitle={subtitle}
      size="lg"
      subheader={
        <ol className="wizard-stepbar" aria-label="Progress">
          <li
            ref={(el) => {
              stepRefs.current[0] = el;
            }}
            className={`wizard-step${stepIndex >= 1 ? " is-active" : ""}${stepIndex > 1 ? " is-done" : ""}`}
          >
            <span className="wizard-step-num">1</span>
            <span className="wizard-step-label">Game</span>
          </li>
          <li
            ref={(el) => {
              stepRefs.current[1] = el;
            }}
            className={`wizard-step${stepIndex >= 2 ? " is-active" : ""}${stepIndex > 2 ? " is-done" : ""}`}
          >
            <span className="wizard-step-num">2</span>
            <span className="wizard-step-label">Source</span>
          </li>
          {showPsarcStep && (
            <li
              ref={(el) => {
                stepRefs.current[2] = el;
              }}
              className={`wizard-step${stepIndex >= 3 ? " is-active" : ""}${stepIndex > 3 ? " is-done" : ""}`}
            >
              <span className="wizard-step-num">3</span>
              <span className="wizard-step-label">Extract</span>
            </li>
          )}
          <li
            ref={(el) => {
              stepRefs.current[showPsarcStep ? 3 : 2] = el;
            }}
            className={`wizard-step${stepIndex >= (showPsarcStep ? 4 : 3) ? " is-active" : ""}`}
          >
            <span className="wizard-step-num">{showPsarcStep ? 4 : 3}</span>
            <span className="wizard-step-label">Open</span>
          </li>
        </ol>
      }
      footer={
        step === "folder" ? (
          <>
            <Button
              onClick={() => setStep(psarcDone ? "psarc" : "source")}
              disabled={busy}
            >
              <ArrowLeft size={12} strokeWidth={2} /> Back
            </Button>
            <Button onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={handleConfirm}
              disabled={!path.trim()}
              loading={busy}
            >
              {busy ? "Loading…" : "Open"}
            </Button>
          </>
        ) : step === "source" ? (
          <>
            <Button onClick={() => setStep("game")}>
              <ArrowLeft size={12} strokeWidth={2} /> Back
            </Button>
            <Button onClick={onClose}>Cancel</Button>
          </>
        ) : step === "psarc" ? (
          <>
            <Button onClick={() => setStep("source")} disabled={psarcBusy}>
              <ArrowLeft size={12} strokeWidth={2} /> Back
            </Button>
            <Button onClick={onClose} disabled={psarcBusy}>
              Cancel
            </Button>
            <Button
              onClick={handleExtractPsarc}
              disabled={!psarcInput.trim() || !psarcOutput.trim() || psarcBusy}
              loading={psarcBusy}
            >
              {psarcBusy ? "Extracting…" : psarcDone ? "Extract another" : "Extract"}
            </Button>
            <Button
              variant="primary"
              onClick={handleContinueAfterExtract}
              disabled={!psarcDone || psarcBusy}
              title={
                psarcDone
                  ? "Move on to picking the level folder"
                  : "Extract at least one archive first"
              }
            >
              Continue →
            </Button>
          </>
        ) : (
          <Button onClick={onClose}>Cancel</Button>
        )
      }
    >
      {step === "game" && (
        <div className="game-franchises">
          <div
            className="game-franchise-tabs"
            role="tablist"
            aria-label="Choose franchise"
          >
            {FRANCHISES.map((f) => {
              const games = GAMES.filter((g) => g.franchise === f.id);
              if (games.length === 0) return null;
              const isActive = franchiseTab === f.id;
              return (
                <button
                  key={f.id}
                  type="button"
                  role="tab"
                  aria-selected={isActive}
                  className={`game-franchise-tab${isActive ? " active" : ""}`}
                  onClick={() => pickFranchise(f.id)}
                >
                  <span className="game-franchise-tab-label">{f.label}</span>
                  <span className="game-franchise-tab-count dim small">
                    {games.length}
                  </span>
                </button>
              );
            })}
          </div>
          {(() => {
            const games = GAMES.filter((g) => g.franchise === franchiseTab);
            return (
              <div className="game-grid">
                {games.map((g) => (
                  <button
                    key={g.id}
                    type="button"
                    className={`game-tile${g.supported ? "" : " is-disabled"}`}
                    onClick={() => g.supported && pickGame(g.id)}
                    disabled={!g.supported}
                    title={
                      g.supported
                        ? `Open ${g.label}`
                        : `${g.short} — parser support is not implemented yet`
                    }
                  >
                    <div className="game-tile-art">
                      <img
                        src={g.logoSrc}
                        alt={g.label}
                        draggable={false}
                        onError={(e) => {
                          (e.currentTarget as HTMLImageElement).style.opacity = "0";
                        }}
                      />
                      {!g.supported && (
                        <div className="game-tile-lockoverlay">
                          <Lock size={20} strokeWidth={2} />
                          <span className="small">Not yet supported</span>
                        </div>
                      )}
                    </div>
                    <div className="game-tile-meta">
                      <div className="game-tile-name">{g.label}</div>
                      <div className="game-tile-byline dim small">{g.byline}</div>
                      <ul className="game-tile-caps" aria-label="Supported features">
                        {CAPABILITY_LABELS.map(({ key, label }) => {
                          const state = g.capabilities[key];
                          return (
                            <li
                              key={key}
                              className={`game-tile-cap is-${state}`}
                              title={capTooltip(label, state)}
                            >
                              <span className="game-tile-cap-dot" aria-hidden />
                              <span className="game-tile-cap-label">{label}</span>
                            </li>
                          );
                        })}
                      </ul>
                    </div>
                  </button>
                ))}
              </div>
            );
          })()}
        </div>
      )}

      {step === "source" && (
        <div className="source-step">
          {selectedGameSpec && (
            <div className="open-level-game-row">
              <span className="game-card-tag mono">{selectedGameSpec.short}</span>
              <span className="small dim">{selectedGameSpec.label}</span>
              <button
                type="button"
                className="btn"
                onClick={() => setStep("game")}
                title="Pick a different game"
              >
                <ArrowLeft size={12} strokeWidth={2} /> Change
              </button>
            </div>
          )}
          <div className="source-info">
            <strong className="small">📦 Before you start</strong>
            <p className="small dim">
              A level needs <em>every</em> <code>.psarc</code> that ships with
              it extracted into <strong>the same folder</strong>.
              {selectedGameSpec && <> {selectedGameSpec.hint}</>} Missing
              siblings show up as "no audio" badges or empty meshes.
            </p>
          </div>
          <div className="source-grid">
            <button
              type="button"
              className="source-card"
              onClick={() => {
                setPath("");
                setWarning(null);
                setPsarcDone(false);
                setStep("folder");
              }}
            >
              <div className="source-card-icon">
                <Folder size={36} strokeWidth={1.4} />
              </div>
              <div className="source-card-title">Open a level folder</div>
              <div className="source-card-sub small dim">
                Already-extracted files. Pick the folder that contains
                <code> {entryFile}</code> and its sibling
                <code> .dat</code> files.
              </div>
            </button>
            <button
              type="button"
              className="source-card"
              onClick={handlePickPsarc}
              title="Extract a .psarc archive"
            >
              <div className="source-card-icon">
                <Package size={36} strokeWidth={1.4} />
              </div>
              <div className="source-card-title">Extract a PSARC</div>
              <div className="source-card-sub small dim">
                Unpack a raw <code>.psarc</code>. Repeat for every
                <code> .psarc</code> belonging to this map and point them all
                at the <strong>same destination folder</strong> — then come
                back here and pick "Open a level folder".
              </div>
            </button>
          </div>
        </div>
      )}

      {step === "psarc" && (
        <div className="open-level">
          {selectedGameSpec && (
            <div className="open-level-game-row">
              <span className="game-card-tag mono">{selectedGameSpec.short}</span>
              <span className="small dim">{selectedGameSpec.label}</span>
              <button
                type="button"
                className="btn"
                onClick={() => setStep("source")}
                title="Back to source choice"
                disabled={psarcBusy}
              >
                <ArrowLeft size={12} strokeWidth={2} /> Change
              </button>
            </div>
          )}
          <div className="source-info">
            <strong className="small">📦 Extract one or more .psarc archives</strong>
            <p className="small dim">
              Different games need different file sets:
            </p>
            <ul className="small dim" style={{ margin: "4px 0 6px 18px", padding: 0 }}>
              <li>
                <strong>R2 / R3 / R&C: FFA / R&C: A4O</strong> — extract both
                <code> level_cached.psarc</code> and <code>level_uncached.psarc</code>{" "}
                into the same folder.
              </li>
              <li>
                <strong>RFOM</strong> — extract <code>game.psarc</code> first, then
                each per-level cache archive.
              </li>
              <li>
                <strong>R&C: ToD</strong> — varies per level; extract whatever
                <code> .psarc</code> files ship with that level.
              </li>
            </ul>
            <p className="small dim">
              Pick a <code>.psarc</code> and an output folder, hit{" "}
              <strong>Extract</strong>, then either pick another archive (the
              output folder stays so files merge in place) or click{" "}
              <strong>Continue →</strong> when you've extracted everything you
              need.
            </p>
          </div>

          <div className="open-level-pickers">
            <button
              type="button"
              className="open-level-card"
              onClick={handleBrowsePsarcInput}
              disabled={psarcBusy}
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
              onClick={handleBrowsePsarcOutput}
              disabled={psarcBusy}
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
              value={psarcInput}
              onChange={(e) => setPsarcInput(e.target.value)}
              placeholder="C:\\path\\to\\archive.psarc"
              spellCheck={false}
              disabled={psarcBusy}
            />
          </label>
          <label className="open-level-field">
            <input
              type="text"
              value={psarcOutput}
              onChange={(e) => setPsarcOutput(e.target.value)}
              placeholder="C:\\path\\to\\output"
              spellCheck={false}
              disabled={psarcBusy}
            />
          </label>

          {psarcError && <div className="error-banner">{psarcError}</div>}

          {psarcProgress && psarcProgress.total > 0 && (
            <div className="psarc-progress" style={{ marginTop: 4 }}>
              <div className="load-progress-bar">
                <div
                  className="load-progress-fill"
                  style={{
                    width: `${
                      Math.min(
                        100,
                        (psarcProgress.current / psarcProgress.total) * 100,
                      )
                    }%`,
                  }}
                />
              </div>
              <div className="psarc-progress-meta">
                <span className="mono small">
                  {psarcProgress.current.toLocaleString()} /{" "}
                  {psarcProgress.total.toLocaleString()}
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
                  {psarcProgress.lastFile}
                </span>
              </div>
            </div>
          )}

          {psarcDone && (
            <div className="open-level-hint small" style={{ borderColor: "rgba(74, 222, 128, 0.4)", background: "rgba(74, 222, 128, 0.06)" }}>
              ✓ Extraction complete. If this game needs more archives (e.g.
              <code> level_uncached.psarc</code> after <code>level_cached.psarc</code>
              for R2 / R3 / V2 games), pick the next <code>.psarc</code> and
              hit <strong>Extract another</strong>. When you've extracted
              everything, click <strong>Continue →</strong>.
            </div>
          )}

          <div className="open-level-hint small dim">
            Supports any ZLIB-compressed PSARC v1.3 or v1.4 archive (most
            PS3-era games). LZMA and OODLE compressions are recognized but
            not yet decoded.
          </div>
        </div>
      )}

      {step === "folder" && (
        <div className={`open-level ${dropPhase === "over" ? "drop-over" : ""}`}>
          {selectedGameSpec && (
            <div className="open-level-game-row">
              <span className="game-card-tag mono">{selectedGameSpec.short}</span>
              <span className="small dim">{selectedGameSpec.label}</span>
              <button
                type="button"
                className="btn"
                onClick={() => setStep("source")}
                title="Back to source choice"
              >
                <ArrowLeft size={12} strokeWidth={2} /> Change
              </button>
            </div>
          )}
          <div className="open-level-droptarget">
            <div className="open-level-droptarget-text">
              {dropPhase === "over"
                ? "Drop to open this level"
                : `Drag a folder or ${entryFile} here`}
            </div>
            <div className="open-level-droptarget-sub small dim">
              All of the level's <code>.psarc</code> archives must already be
              extracted into this same folder.
            </div>
          </div>
          <div className="open-level-pickers">
            <button
              type="button"
              className="open-level-card"
              onClick={handleBrowseFolder}
              disabled={busy}
            >
              <div className="open-level-card-icon" aria-hidden>
                <Folder size={28} strokeWidth={1.5} />
              </div>
              <div className="open-level-card-text">
                <div className="open-level-card-title">Pick a folder</div>
                <div className="open-level-card-sub small dim">
                  Select the directory directly
                </div>
              </div>
            </button>
            <button
              type="button"
              className="open-level-card"
              onClick={handleBrowseFile}
              disabled={busy}
            >
              <div className="open-level-card-icon" aria-hidden>
                <File size={28} strokeWidth={1.5} />
              </div>
              <div className="open-level-card-text">
                <div className="open-level-card-title">
                  Pick <code>{entryFile}</code>
                </div>
                <div className="open-level-card-sub small dim">
                  We'll use the parent folder
                </div>
              </div>
            </button>
          </div>

          <label className="open-level-field">
            <span className="open-level-field-label small dim">
              Or paste a path
            </span>
            <input
              type="text"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleConfirm();
              }}
              placeholder="C:\\path\\to\\level"
              spellCheck={false}
              disabled={busy}
            />
          </label>

          {warning && <div className="open-level-warning">{warning}</div>}

          {recent.length > 0 && (
            <div className="open-level-recent">
              <div className="open-level-section-title small dim">Recent</div>
              <ul className="open-level-recent-list">
                {recent.map((folder) => {
                  const candidates = game
                    ? levelThumbCandidatesFromPath(folder, game)
                    : [];
                  return (
                  <li key={folder} className="open-level-recent-item">
                    <button
                      type="button"
                      className="open-level-recent-btn"
                      onClick={() => confirm(folder)}
                      disabled={busy}
                      title={folder}
                    >
                      <FallbackImage
                        candidates={candidates}
                        alt=""
                        className="open-level-recent-thumb"
                        placeholder={game ? franchisePlaceholder(game) : undefined}
                        placeholderLabel="No image"
                      />
                      <span className="open-level-recent-name">
                        {lastTwoSegments(folder)}
                      </span>
                      <span className="open-level-recent-path mono small dim">
                        {folder}
                      </span>
                    </button>
                    <button
                      type="button"
                      className="open-level-recent-remove"
                      onClick={() => removeRecent(folder)}
                      title="Remove from recent"
                      aria-label="Remove"
                    >
                      <X size={14} strokeWidth={2} />
                    </button>
                  </li>
                  );
                })}
              </ul>
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}
