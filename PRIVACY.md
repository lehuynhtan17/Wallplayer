# Privacy policy

Wallplayer has no account of its own, no analytics and no telemetry. It does not
collect information about you and does not send anything to the Wallplayer authors.

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it, with the
one exception below, which you can turn off.

## What it contacts on its own

| When | Service | What is sent | Turn it off |
|---|---|---|---|
| Once when Wallplayer starts | GitHub (`api.github.com`) | A request for the latest release, with the app version in the user agent | Settings → General → *Check when Wallplayer starts* |

## What it contacts because you use a feature

| Feature | Services | What is sent |
|---|---|---|
| Search, artist pages, suggestions, radio, *Music for you* (YouTube Home → Music, with your YouTube sign-in) | YouTube Music / YouTube, SoundCloud, Spotify, Apple (iTunes Search) | Your search text, the song or artist you open |
| Playing music and music videos | YouTube (embedded player), SoundCloud (embedded player) | What you play, as with the official players |
| TikTok channel (platform button → TikTok) | TikTok (its own web page, opened out of sight, and its official embed player) | What TikTok's site receives when you open For You or search on it: your TikTok session if you are signed in, your search text, the videos you watch. Wallplayer only reads the list of videos (and a video's comments) from that page. Like and Save press TikTok's own buttons on the video's TikTok page, with your account, only when you click them |
| Signing in to YouTube, SoundCloud, Spotify or TikTok | Google, SoundCloud, Spotify, TikTok sign-in pages | What you type on their pages. Sign-in cookies stay in Wallplayer's local WebView2 profile on your PC |
| Synced lyrics, and word-by-word timing (Settings → Lyrics → *Find lyrics online*, on by default) | LRCLIB, NetEase Music, Genius | Title, artist and length of the song playing |
| Second-language lyrics | Google Translate | The lyric lines being translated |
| Fonts in the editor | Google Fonts | A request for the font files |
| Star on GitHub (Settings → General → Support, or the reminder card) | GitHub (`github.com`, in a window you sign in to yourself; the sign-in cookie stays in Wallplayer's local profile) | What GitHub's site receives when you sign in and press Star: your GitHub session. Nothing is sent when you don't press it. The reminder only counts days of use on your PC |
| Updating | GitHub | Downloads the release zip, only after you choose *Update* |

Each of these services has its own privacy policy, which applies to what you send them.

## What stays on your PC

Settings, playback history and the sign-in cookies above are stored only on your PC,
in Wallplayer's folders under `%APPDATA%` and `%LOCALAPPDATA%` (`com.tan.wallplayer`).
Deleting those folders removes them.

## Changes on your PC

- **Hide desktop icons** (on by default, tray menu): the icons are hidden while Wallplayer runs and come back when it quits.
- **Start with Windows** (off by default, tray menu): adds a `Wallplayer` entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- **Updating**: replaces `Wallplayer.exe` in its own folder; the old file is kept as `Wallplayer.exe.old` until the next start.
