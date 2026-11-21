<p align="center">
  <img src="docs/thumbnail.png" alt="Wallplayer" width="100%">
</p>

<h3 align="center">Your wallpaper, playing.</h3>

<p align="center">
  A glass music skin for the Windows desktop — the wallpaper itself becomes the player.<br>
  <sub>Tall clock · glass cover flow · synced lyrics with translations · an edge light that moves with the music</sub>
</p>

<p align="center">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%20%7C%2011-6366f1?style=flat-square">
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-v2-8e94ff?style=flat-square">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-%F0%9F%A6%80-4f46e5?style=flat-square">
  <img alt="License: MIT" src="https://img.shields.io/badge/License-MIT-a5b4fc?style=flat-square">
</p>

---

## What it feels like

Wallplayer lives *behind* your desktop icons, the same way Wallpaper Engine does, so **Win + D keeps it on screen**. A tall hollow clock, a glass cover flow of what is playing, a “Continue Playing” queue and a neon edge light that follows the loudness of whatever your PC is playing.

| | |
|---|---|
| **Smart search** | Songs, **Singers** and **Playlists** in one box — tabs *All · Songs · Singers · Playlists*, typing suggestions, YouTube Music, SoundCloud and Spotify. Opened empty, it shows **Music for you**: the playlists YouTube picks for your account under Home → Music. |
| **Continue Playing** | Pick one song and the queue becomes YouTube’s own **Mix** for it (personal when signed in), and it keeps going like on YouTube. A **History** button flips it to *History Playing* (last 100 songs). |
| **TikTok** | A TikTok channel made for it: one vertical video at a time, scroll or ↑ ↓ for the next, *Auto scroll* on or off, *For You* (yours when signed in) or any search, with *Up next*, the video's details, and Like · Comments · Save · Share beside it. |
| **Synced lyrics** | Found automatically online (LRCLIB, NetEase, Genius), timed to the song — **word by word** when NetEase has it — with a second-language subtitle line and per-song offset (`[` `]`). 14 effects, including an **Apple Music**-style scrolling view. |
| **Ambient light** | WebGL neon edge that reacts to the system audio — 11 effects (Comet, Pulse, Spectrum, Rainbow, Aurora, Fire, Lava, Snake, Sparkle, Neon, Wave) on the whole frame, the bottom, the top or the corners, per display — coloured from the album art. The tray icon becomes the album cover with a beating ring. |
| **Backgrounds** | Music video, your own video, “Surprise me” (no AI-generated clips), or your Windows wallpaper between songs. |
| **Lock-screen style editor** | Fonts, glass and colours for the clock and lyrics, like the iPhone lock screen. |
| **Search from anywhere** | The one global shortcut — default `Ctrl + Alt + Space`, rebind it to any key combo **or mouse button**. |
| **Light on your PC** | Stops drawing when the music stops, rests behind full-screen and maximised windows, gets lighter on battery (Auto / Full / Saver). |
| **Updates itself** | No installer: one quiet check at start; a new version shows up as *Update available* in the tray menu, with the release notes and *Update* / *Skip*. |
| **Your own songs** | Add a music folder or drop songs on the window; tags and covers are read from the files. |
| **Two languages** | English and Vietnamese interface (Settings → General → Language). |
| **Desktop, tidied** | Auto-hides desktop icons (Recycle Bin included) and gives them back when you quit. Multi-monitor and portrait screens supported. |

## Download

Download `Wallplayer-Windows-x64.zip` from [Releases](https://github.com/lehuynhtan17/Wallplayer/releases).
Release notes for each version are in [`CHANGELOG.md`](CHANGELOG.md).
See the [Code signing policy](CODE_SIGNING.md).

## Quick start

1. Unzip `Wallplayer-Windows-x64.zip` and keep the files in the `Wallplayer` folder together.
2. Run `Wallplayer.exe`. It needs the Microsoft Edge **WebView2 Runtime**, which Windows 11 and most Windows 10 PCs already have.
3. Press **Ctrl + Alt + Space** and search. Everything else is in the tray icon and in Settings (the ··· button at the end of the player bar). [`GUIDE.txt`](GUIDE.txt) is the full user guide ([Vietnamese](HUONG-DAN.txt)).

> Press **Ctrl + Shift + D** to run the built-in self-test if something Windows-specific misbehaves.

**Updating:** Wallplayer checks for a new version once when it starts, quietly. When there is one, the tray menu shows *Update available*; click it to read the release notes and choose *Update* or *Skip*. Nothing is installed: the new `Wallplayer.exe` replaces the old one in the same folder.

**Uninstalling:** in the tray menu, untick *Start with Windows*, then choose *Quit Wallplayer*. Delete the `Wallplayer` folder. To remove settings and sign-ins too, delete the `com.tan.wallplayer` folders in `%APPDATA%` and `%LOCALAPPDATA%`.

## Architecture

| Layer | Where | What |
|---|---|---|
| Window placement | `src-tauri/src/desktop.rs` | Borderless full-screen window pinned to the bottom of the z-order (subclassed `WM_WINDOWPOSCHANGING`), or re-parented into the shell `WorkerW` behind the icons (classic + Win11 24H2 layouts). Peek mode = top-most for the hotkey search. |
| Now Playing | `src-tauri/src/media.rs` | `GlobalSystemMediaTransportControlsSessionManager` (WinRT): title/artist/album/thumbnail/timeline of Spotify, Apple Music, browsers…; play/pause/next/prev/seek/shuffle. Emits `media`, `media-art`. |
| Ambient spectrum | `src-tauri/src/audio.rs` | WASAPI loopback via `cpal` + 4096-pt FFT (`rustfft`) → `audio-spectrum` [rms, 32 log bands] ≈60 Hz. UI does kick/snare onset detection and drives a WebGL neon-edge shader coloured by the album accent. |
| Accounts | `src-tauri/src/accounts.rs` | YouTube / SoundCloud sign-in windows sharing the WebView2 profile (embedded players run signed-in → Premium/Go+); Spotify OAuth PKCE (user's Client ID) for search + Spotify Connect playback. |
| Network | `src-tauri/src/net.rs` | Per-platform search: YouTube Music (InnerTube WEB_REMIX), YouTube (InnerTube WEB), SoundCloud (api-v2), Apple (iTunes Search); MV ids, LRCLIB synced lyrics, image → data URL. |
| Shell | `src-tauri/src/main.rs` | Tray menu, one rebindable global shortcut (key or mouse button, default Ctrl+Alt+Space), settings, autostart (HKCU Run). |
| UI | `ui/index.html` | Single-file UI. Runs standalone in a browser in demo mode (generated covers, simulated playback). |

Platform behaviour: YouTube Music/Automatic play in-app through the YouTube IFrame player
(video doubles as wallpaper); Spotify opens `spotify:search:`; Apple Music, Spotify Web
Player, SoundCloud and Amazon Music open their search/track page. Whatever plays is picked
up again through Windows media sessions.

## Build

On Windows (MSVC toolchain, recommended):

```
cd src-tauri
cargo build --release
```

Output: `src-tauri/target/release/wallplayer.exe` (WebView2 loader is linked statically on MSVC).

Cross-compiling from Linux (for quick tests; releases are built by GitHub Actions):

```
# needs: mingw-w64, rust-std for x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
# ship wallplayer.exe together with target/x86_64-pc-windows-gnu/release/WebView2Loader.dll
```

Preview the UI without building: open `ui/index.html` in Chrome/Edge.

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).
Committers and reviewers: [lehuynhtan17](https://github.com/lehuynhtan17). Approvers: [lehuynhtan17](https://github.com/lehuynhtan17).
Details in [`CODE_SIGNING.md`](CODE_SIGNING.md).

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it. The one exception, a check for a new version when it starts, can be turned off. Each online feature and the service it uses is listed in the [privacy policy](PRIVACY.md).

## Releasing

1. Write the release notes under a new `## x.y.z` heading in `CHANGELOG.md` and set the same version in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
2. Push to `main`. GitHub Actions builds `Wallplayer.exe`; download the build from the run to test it on Windows.
3. Open **Actions → Build and release → Run workflow**, tick *Publish a release* (or push a tag `Vx.y.z`). The workflow builds again, sends the exe to SignPath for signing (once it is set up), waits for the approval, and publishes release `Vx.y.z` with the notes.

## License

[MIT](LICENSE). Bundled fonts (Inter, Nunito, Playfair Display, Big Shoulders Display, Anton, Space Mono, Roboto) are under the SIL Open Font License, see [`ui/fonts/LICENSES.txt`](ui/fonts/LICENSES.txt).

## Notes

- Genius, YouTube InnerTube and Google Translate use unofficial endpoints and may change at any time.
- Google lyrics cannot be automated (captcha); the app only opens the browser.
- Design thumbnail: [`docs/thumbnail.png`](docs/thumbnail.png).
