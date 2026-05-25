import { useEffect, useRef, useState } from "react";
import type { ExtractedSound } from "../api";

interface SoundPlayerProps {
  /** Null = render the empty placeholder. Set = bind to that audio. */
  nowPlaying: NowPlaying | null;

  onLog?: (level: "info" | "ok" | "warn" | "error", text: string) => void;
}

export interface NowPlaying {
  name: string;
  source: string;
  audio: HTMLAudioElement;
  blobUrl: string;
  entry: ExtractedSound;
}

function fmtTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const total = Math.floor(seconds);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

// Why: volume is a personal-preference setting that should persist across
// app launches AND apply to every track the user plays in this session.
// Keeping it in localStorage means the next-loaded audio element inherits
// the user's last choice without them touching the slider again.
const VOLUME_STORAGE_KEY = "rechimera.soundPlayerVolume";

function loadStoredVolume(): number {
  try {
    const v = localStorage.getItem(VOLUME_STORAGE_KEY);
    if (v == null) return 1;
    const n = Number(v);
    return Number.isFinite(n) ? Math.max(0, Math.min(1, n)) : 1;
  } catch {
    return 1;
  }
}

function storeVolume(v: number): void {
  try {
    localStorage.setItem(VOLUME_STORAGE_KEY, String(v));
  } catch {
    /* ignore quota / disabled storage */
  }
}

export function SoundPlayer({ nowPlaying, onLog }: SoundPlayerProps) {
  const [paused, setPaused] = useState(true);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [volume, setVolume] = useState<number>(() => loadStoredVolume());

  const seekingRef = useRef(false);
  const audio = nowPlaying?.audio ?? null;

  // Every time a new audio element comes in, inherit the user's
  // persisted volume immediately — no waiting for the slider event.
  useEffect(() => {
    if (audio) audio.volume = volume;
  }, [audio, volume]);

  useEffect(() => {
    if (!audio) {
      setPaused(true);
      setCurrentTime(0);
      setDuration(0);
      return;
    }
    const onPlay = () => setPaused(false);
    const onPause = () => setPaused(true);
    const onTimeUpdate = () => {
      if (!seekingRef.current) setCurrentTime(audio.currentTime);
    };
    const onDurationChange = () => setDuration(audio.duration || 0);
    const onEnded = () => {
      setPaused(true);
      setCurrentTime(0);
    };

    audio.addEventListener("play", onPlay);
    audio.addEventListener("pause", onPause);
    audio.addEventListener("timeupdate", onTimeUpdate);
    audio.addEventListener("durationchange", onDurationChange);
    audio.addEventListener("ended", onEnded);

    setPaused(audio.paused);
    setCurrentTime(audio.currentTime);
    setDuration(audio.duration || 0);

    return () => {
      audio.removeEventListener("play", onPlay);
      audio.removeEventListener("pause", onPause);
      audio.removeEventListener("timeupdate", onTimeUpdate);
      audio.removeEventListener("durationchange", onDurationChange);
      audio.removeEventListener("ended", onEnded);
    };
  }, [audio]);

  const togglePlay = () => {
    if (!audio) return;
    if (audio.paused) {
      audio.play().catch((e) => onLog?.("error", `Audio play failed: ${e}`));
    } else {
      audio.pause();
    }
  };

  const onSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!audio || !Number.isFinite(audio.duration)) return;
    const t = Number(e.target.value);
    seekingRef.current = true;
    audio.currentTime = t;
    setCurrentTime(t);
  };
  const onSeekEnd = () => {
    seekingRef.current = false;
  };

  const onVolume = (e: React.ChangeEvent<HTMLInputElement>) => {
    const v = Number(e.target.value);
    setVolume(v);
    storeVolume(v);
    if (audio) audio.volume = v;
  };

  const progressMax = duration > 0 ? duration : 1;
  const isEmpty = !nowPlaying;
  const channels = nowPlaying?.entry.channels ?? 0;
  const channelLabel = isEmpty
    ? ""
    : channels === 1
      ? "mono"
      : channels === 2
        ? "stereo"
        : `${channels}ch`;
  const meta = isEmpty
    ? "Select a sound to play"
    : `${nowPlaying!.source} · ${channelLabel} · ${nowPlaying!.entry.sample_rate} Hz`;

  return (
    <div
      className={`sound-player ${isEmpty ? "empty" : ""}`}
      role="region"
      aria-label="Sound player"
    >
      <button
        type="button"
        className="sp-toggle"
        onClick={togglePlay}
        disabled={isEmpty}
        title={isEmpty ? "Nothing loaded" : paused ? "Play" : "Pause"}
      >
        {paused ? "▶" : "❚❚"}
      </button>
      <div className="sp-info">
        <div className="sp-name" title={nowPlaying?.name ?? ""}>
          {nowPlaying?.name ?? <span className="dim">—</span>}
        </div>
        <div className="sp-meta" title={meta}>
          {meta}
        </div>
      </div>
      <span className="sp-time mono small">{fmtTime(currentTime)}</span>
      <input
        type="range"
        className="sp-progress"
        min={0}
        max={progressMax}
        step={0.01}
        value={Math.min(currentTime, progressMax)}
        onChange={onSeek}
        onMouseUp={onSeekEnd}
        onTouchEnd={onSeekEnd}
        disabled={isEmpty || !Number.isFinite(duration) || duration === 0}
      />
      <span className="sp-time mono small">{fmtTime(duration)}</span>
      <span className="sp-volume-icon" aria-hidden>
        {volume === 0 ? "🔇" : volume < 0.5 ? "🔉" : "🔊"}
      </span>
      <input
        type="range"
        className="sp-volume"
        min={0}
        max={1}
        step={0.01}
        value={volume}
        onChange={onVolume}
        title={`Volume ${Math.round(volume * 100)}% (shared across all sounds)`}
      />
    </div>
  );
}
