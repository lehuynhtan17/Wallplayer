# Changelog

Each section is the release notes of one version. The release workflow publishes
the section that matches the version in `src-tauri/Cargo.toml`.

## 1.0.19

### Fixed
- TikTok (1.0.18): the whole screen looked washed out, as if under a white haze, and on a landscape display the TikTok layout was drawn too big, so the buttons beside the video could end up under the search field. Both came from one mistake in 1.0.18.
- TikTok: a long caption scrolls inside its box instead of running into the search field and player bar (portrait) or off the screen (landscape). The buttons beside the video shrink if the screen is too short for all of them.
- TikTok on a portrait display: the clock sits small in the space above the video instead of touching it.

## 1.0.18

### New
- The other displays can be clicked: pick a song in their playlist or cover flow and it plays on the main display. Works on Windows, on Linux, and on macOS when Wallplayer sits over the Finder icons.

### Changed
- TikTok on a portrait (upright) display: the video and its buttons are bigger, and the video's details and Up next sit under the video instead of beside it.
- TikTok on the other displays: only the blurred picture of the video playing on the main display shows there, without the still of the video, its buttons, details or the clock.

## 1.0.17

### Changed
- Wallplayer no longer picks up music or videos playing in other apps by itself. A Facebook or YouTube video in the browser, a game, or music you start in another app no longer brings its cover, lyrics or music video into the player, and no longer lands in History. Only a song or playlist you start from Wallplayer that plays in Spotify, Spotify Web or Apple Music shows in the player (and Spotify's next songs after it), until you play something else in Wallplayer.
- History drops the songs that came from other apps that way before this version.

## 1.0.16

### New
- Linux and macOS preview builds. Each release now has one zip per system: `Wallplayer-Windows-x64.zip`, `Wallplayer-Linux-x64.zip` and `Wallplayer-macOS.zip`. The Linux and macOS builds have been tested on GitHub's Linux and Mac machines but not yet on real PCs or Macs; "Linux and macOS" in GUIDE.txt lists what differs from Windows.
  - Linux (64-bit): Wallplayer becomes the desktop (on Wayland through XWayland), music playing in other apps comes in through MPRIS, and the system volume and ambient light go through PipeWire / PulseAudio. Needs WebKitGTK 4.1 (Ubuntu 22.04 or newer).
  - macOS (Apple silicon and Intel): Wallplayer sits on the desktop of every Space, Spotify and Apple Music show in the player, and the search shortcut is ⇧⌘Space. The app is not notarized by Apple: the first time, right-click it → Open.
- Nothing changes on Windows apart from the fix below.

### Fixed
- The music video wallpaper could run up to 2.5 seconds ahead of or behind the song. It is now kept within about a tenth of a second (half a second for Spotify and other apps, which report their position less precisely): small drift is caught up by playing the video slightly faster or slower, without a visible jump, and a big gap is jumped over. Videos on the other displays follow the main one the same way.

## 1.0.15

### Changed
- Portrait screens: lyrics sit right under the cover flow when the position is Center, Left or Right, and under the playlist when it is Bottom; the playlist moves down to make room, and its highlight has more space.

## 1.0.14

### Fixed
- "Check for updates" could fail with "GitHub answered 403 Forbidden". GitHub's API answers only 60 questions an hour per internet address without an account, and many people can share one address (a VPN such as Cloudflare WARP, a mobile network, an office). Wallplayer now falls back to the release page, which has no such limit.

## 1.0.13

### New
- Settings → Performance → "High priority": Wallplayer opens as soon as you sign in, before the other startup apps, instead of waiting for its turn in Windows' startup list. It uses a Task Scheduler task of your own account, so no administrator rights are needed.

### Changed
- Started with Windows, Wallplayer and its web engine run at high priority until the skin is up, then go back to normal so games and other apps aren't slowed down.
- Started at sign-in before the desktop is ready, the skin stays hidden until it can go behind the icons, instead of showing up as a plain window.

## 1.0.12

### Fixed
- "Start with Windows" could be on in Wallplayer while Windows never started it: the setting pointed to an older copy or folder, or it had been switched off in Windows' own list (Task Manager → Startup apps). Wallplayer now shows it on only when Windows will really start this copy, points it to the current folder by itself at every start, and turning it on also switches it back on in Windows' list.
- Started at sign-in, Wallplayer waits until the desktop is ready (taskbar and icons are up) before going behind the icons, instead of trying too early and staying a plain window.

### Changed
- Releases are not code-signed for now (the SignPath Foundation programme declined the project). README and the guides explain the SmartScreen warning and how to unblock the exe.

## 1.0.11

### New
- Click the search field with nothing typed and, when you are signed in to YouTube, **Music for you** shows the playlists YouTube picks for you under Home → Music (your Mixes, *My Mix*, artist mixes), with the rows YouTube gives them. Click one to play it: a Mix keeps going like on YouTube. It stays fresh for 15 minutes and reloads in the background; signed out, a small card offers to sign in (it can be hidden).

### Fixed
- The skin no longer disappears when Windows rebuilds the desktop's wallpaper layer (often when a game starts or changes display mode, the wallpaper changes or the screen wakes up). On Windows 11 24H2 and later the skin now sits directly under the desktop icons, as Microsoft asks of wallpaper apps (and as Lively Wallpaper does), instead of inside the layer Windows rebuilds; when that layer comes back, Wallplayer puts it under the skin again.
- When the skin window did go away, Wallplayer closed itself before it could start again. It now stays in the tray and starts again.

## 1.0.10

### Fixed
- Wallplayer seemed to crash: the skin vanished and only the tray icon was left. The skin lives inside the desktop's wallpaper layer, and when Windows rebuilds that layer (a wallpaper change, the screen waking up, a full-screen game switching display modes, Explorer restarting…) the skin window went with it. Wallplayer now notices and starts itself again (at most 3 times in 10 minutes), and writes it to `wallplayer.log`.

## 1.0.9

### New
- TikTok: *See more* under a long caption shows all of it (*See less* folds it again).

### Fixed
- Other displays now follow TikTok too: instead of the playlist they show the same video (as a still frame), its Like / Save state, the tabs, *Up next* and the video's details. Back to music, they show the playlist again.
- TikTok videos load faster and no longer stutter: the hidden TikTok pages that supply the list stop downloading and playing videos and pictures, the next video loads quietly while the current one plays (so it starts at once), the blurred background is a still frame of the video instead of a second copy playing, and other displays show a still frame instead of another stream.

## 1.0.8

### Changed
- TikTok: the video is a moderate size again, centred in the space above the player, and the search field and player bar sit just above the Windows taskbar instead of under it.
- TikTok: the background is the video itself — a muted, blurred, enlarged copy fills the whole screen and glows out from the frame, like YouTube's ambient mode (a still blurred cover on battery saver).

### Fixed
- TikTok: Like, Save and Comments did nothing. Wallplayer opened the video's TikTok page in a narrow hidden window, where TikTok pushes those buttons off the edge and never opens the comments. The page now opens at desktop width, on the right video, and the comments are loaded from their own tab. If TikTok asks for a "slide the puzzle" check, its window comes up so you can answer it.
- Buttons beside the TikTok video could miss a click while the list of videos was growing.

## 1.0.7

### New
- **YouTube Mix**: click one YouTube song (in the search results or in *History Playing*) and the playlist on the left becomes the Mix YouTube makes for that song — personal when you are signed in to YouTube. It takes the place of *Continue Playing* and keeps going: near the end, the next part of the same Mix is added.
- Settings → Now Playing → **When you pick a song**: *YouTube Mix* (default) or *Mix + your picks* (the previous behaviour).
- **TikTok channel** (platform button → TikTok): one vertical video at a time in the middle of the screen, like the app — no playlist, the clock moves to the left. Scroll over the video, press ↑ / ↓ or use the ▲ ▼ buttons for the previous / next video; click it to pause. **Auto scroll** (under ▼) plays the next video when one ends, or loops each video when off. *For You* is your own feed when you are signed in to TikTok (Settings → Accounts → TikTok); press Enter in the search field to watch a search instead. *Up next* and the video's author, caption and sound sit on the right. Beside the video, like TikTok: **Like**, **Comments** (they open on the right), **Save** to your TikTok Favorites and **Share** (copies the link); Like and Save use your TikTok account (sign in first). Videos play in TikTok's official embed player; the list comes from TikTok's own page, opened out of sight with your session.

### Fixed
- A music video in a YouTube list no longer shows its view count as the album.
- In a long playlist, the rows around the playing song are shown (the list used to stop at the first 100).

## 1.0.6

### Fixed
- Wallplayer seemed to quit by itself after a while: when the Microsoft Edge WebView2 engine behind the skin stopped, the screen went back to the plain wallpaper while Wallplayer stayed in the tray with nothing to show. Wallplayer now starts itself again when that happens (at most 3 times in 10 minutes), and reloads a page whose process stops or hangs. What happened is written to `wallplayer.log` in `%LOCALAPPDATA%\com.tan.wallplayer`.
- The cause on one PC: signing in to YouTube. When the sign-in finished, Wallplayer closed the sign-in window while its page was still busy, and the WebView2 engine (shared by every Wallplayer window) crashed. Sign-in windows (YouTube, SoundCloud, Spotify) and the windows of other displays are now hidden and reused instead of closed, and Alt+F4 no longer closes the skin.

## 1.0.5

### Changed
- Music keeps playing: Wallplayer no longer pauses other apps when it starts a song, no longer asks which one to keep when music plays in two places, and music in another app never stops or replaces the song playing in Wallplayer. When Wallplayer's song has stayed paused for a minute while another app plays, the player shows that app's song.

## 1.0.4

### New
- **Recent playlists:** with nothing typed, the search shows the playlists and albums you played lately under *Recently played*.
- **Save to YouTube Music** (signed in): the heart on a YouTube Music playlist or album card saves it to your library; the + button above *Continue Playing* saves the songs on the left as a new private playlist.
- **Start with Windows** in Settings → General (the same switch as in the tray menu).

### Fixed
- Settings: buttons and switches inside a page did nothing.
- When the playlist moves on to the next song by itself, it no longer pauses a video or song you started elsewhere in the meantime, and a song that is still loading is no longer replaced by another app's media.
- Playlist: the left edge of the highlighted song (and of the song you point at) was cut off; the rounded highlight now shows whole.

## 1.0.3

### New
- **Lyrics word by word.** When NetEase has the timing of every word, the words light up exactly as they are sung, in every lyrics effect. Local `.lrc` lyrics with a time before each word work too.
- **Apple Music lyrics effect.** The song as a column: the line being sung is bright and fills in word by word, the others are dim and softly blurred, the column glides up line by line, and long pauses show three breathing dots. Settings → Lyrics → Effect.
- **Settings window** like macOS System Settings: pages on the left with a search field, settings on the right. It opens from the ··· button at the end of the player bar and brings together the old Appearance panel and the ··· menu.
- **Updates without an installer.** Wallplayer quietly checks for a new version once when it starts. When there is one, the tray menu shows *Update available*; click it to read the release notes and choose *Update* or *Skip*. Turn the check off in Settings → General.
- **Lighter on your computer.** When the music stops, the edge light and background slow down and stop drawing. Behind a full-screen game or film, or a maximised window, Wallplayer rests and pauses the background video. On battery or with Energy Saver it gets lighter by itself (30 fps, video up to 720p). Choose Auto, Full or Saver in Settings → Performance.
- **Your own music.** The folder button on the playlist → *Add a folder…*, or drop folders and songs onto Wallplayer. Titles, artists and covers are read from the files; lyrics and the edge light work as usual. Folders are remembered.
- **Vietnamese interface.** Choose English or Tiếng Việt in Settings → General; the tray menu follows. Wallplayer uses Vietnamese when Windows does.
- **Playlist:** point at a song to play it next or remove it; drag the ⋮⋮ handle to reorder.
- **Lyrics:** timing for this song or for every song in the Lyrics panel, remembered; type to search for other lyrics; long instrumental parts show a ♪ with dots counting down to the next line.
- **Edge light:** 6 new effects (Fire, Lava, Snake, Sparkle, Neon, Wave); light the whole frame, the bottom, the top or only the corners; turn it off on any display; point at an effect to preview it.
- **Tray icon** shows the cover of the song playing, with a ring that beats (Settings → Layout).
- **Pick the video:** dots above the search field choose the song's own music video or another one by the same artist.
- **Music in two places** (Wallplayer and Spotify, for example): Wallplayer asks which one to keep and pauses the other.
- **Turn off lyrics lookups** in Settings → Lyrics → *Find lyrics online*.

### Changed
- Player bar: the cover-flow button now sits between Lyrics and Playlist, with its own icon (lit while the cover flow shows); ··· moved to the end, next to the volume.
- Search field: a ✕ button clears what you typed. The button that hides the interface now has a collapse icon.
- Wallplayer is now open source under the MIT license.
- Fonts come with the app: it no longer loads them from Google at start.
- Dropping a picture or video onto Wallplayer to make it the wallpaper works on Windows.
- The sound analysis skips silence.
- English user guide (`GUIDE.txt`) next to the Vietnamese one.

## 1.0.2 and earlier

See [Releases](https://github.com/lehuynhtan17/Wallplayer/releases).
