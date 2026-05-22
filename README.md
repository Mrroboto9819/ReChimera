<p align="center">
  <img src="apps/desktop/icon.png" width="140" alt="ReChimera logo" />
</p>

<h1 align="center">ReChimera</h1>

<p align="center">
  <strong>Offline level inspector and asset extractor for Insomniac Games' PS3 titles</strong><br/>
  <sub>Resistance: Fall of Man · Resistance 2 · Resistance 3 · Ratchet &amp; Clank: Tools of Destruction · A Crack in Time · All 4 One</sub>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue?style=for-the-badge" alt="License" /></a>
  <a href="https://github.com/Mrroboto9819/ReChimera/releases"><img src="https://img.shields.io/badge/status-beta-orange?style=for-the-badge" alt="Status" /></a>
</p>

<img src="assets/demo.png" alt="ReChimera demo" width="100%" />

---

ReChimera loads a level folder, decodes its meshes / textures / skeletons / animations / sound banks, and lets you preview and export them through a desktop UI. **Use it on game files you legally own.** No game data is shipped.

Deep documentation lives in [`docs/`](docs/) and inside the running app under **Help → Documentation**.

## Use

1. Grab the latest installer from [Releases](https://github.com/Mrroboto9819/ReChimera/releases). Windows auto-updates; macOS / Linux: replace the binary manually.
2. Extract every `.psarc` that ships with your level into a single folder (use any PSARC extractor, or the wizard's built-in **Extract a PSARC** step).
3. Point ReChimera at that folder — it auto-detects the engine era from the layout marker (`assetlookup.dat`, `ps3levelmain.dat`, or `main.dat`).

Missing PSARC siblings don't crash the loader — they show up as `no audio` badges or empty meshes.

## Build from source

Requires **Rust 1.75+** (Edition 2021), **[Bun](https://bun.sh)** (or `npm 10+`), and **WebView2** on Windows (preinstalled on Win11).

```sh
cd apps/desktop
bun install

bun run tauri:dev      # dev window with hot-reload
bun run tauri:build    # installers into src-tauri/target/release/bundle/
```

## License

**GPL-3.0-or-later** — see [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). Built on the format research from [ReLunacy](https://github.com/VELD-Dev/ReLunacy) ([@VELD-Dev](https://github.com/VELD-Dev)), [Lunacy / 7th igRewrite](https://github.com/NefariousTechSupport/7thigRewrite) ([@NefariousTechSupport](https://github.com/NefariousTechSupport)), and [InsomniaToolset](https://github.com/PredatorCZ/InsomniaToolset) ([@PredatorCZ](https://github.com/PredatorCZ)).
