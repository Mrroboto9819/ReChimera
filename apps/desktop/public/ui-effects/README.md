# UI sound effects per skin

Each subfolder maps to a skin id from `apps/desktop/src/store.ts` (`APP_SKINS`).
Drop audio files in matching subfolders and they play automatically while
that skin is active. Missing files are no-ops — there's no need to fill
every slot.

## Folder layout

```
ui-effects/
├── default/
├── resistance1/   ← Resistance: Fall of Man
├── resistance2/   ← Resistance 2
├── resistance3/   ← Resistance 3
└── racit/         ← Ratchet & Clank: A Crack in Time
```

## Recognized event filenames

Inside each skin folder, the runtime looks for these names (any of `.wav`,
`.ogg`, `.mp3` will work — first one found wins):

| File | When it plays |
|---|---|
| `skin-switch` | The moment the user switches **to** this skin |
| `select` | Any `<button>` / `.btn` click while this skin is active |
| `hover` | `<button>` / `.btn` mouseenter — disabled by default in settings |
| `back` | Modal close, escape key dismissals |
| `confirm` | `.btn-primary` clicks |
| `error` | Disabled-button clicks, error toasts |

## Volume + on/off

User controls live in Settings → General → "UI sound effects" toggle and
volume slider. The runtime respects both — when the user disables sound,
nothing fires regardless of files present.

## Where files go on disk

Any file you drop here is served from the Vite dev server at
`/ui-effects/<skin>/<file>` and bundled into the production build by
`vite build`. **You're responsible for using audio you have the right
to distribute** (original recordings, CC0/CC-BY tracks, licensed SFX
libraries, etc.). Don't commit copyrighted game audio.
