# Wallplayer — ghi chú cho Claude Code

Chủ dự án: Tấn (Senior UX/UI Product Designer, không biết code). **Luôn trả lời bằng tiếng Việt**, giải thích đơn giản, tự làm hết các bước (build, commit, push), chỉ hỏi lại khi thật sự cần. **Không tự ý phát hành (publish release): chỉ publish khi Tấn ra lệnh.**

## Dự án
App Windows (đang port thử sang Linux và macOS) (Tauri v2 + Rust) biến hình nền desktop thành trình phát nhạc kiểu "aesthetic desktop skin" (đồng hồ cao, cover flow kính, search nhiều nền tảng, playlist 3D, lyrics đồng bộ + phụ đề 2 ngôn ngữ, ambient light viền neon nhảy theo nhạc, chỉnh đồng hồ/lyrics kiểu lock screen iPhone với chữ kính).

## Cấu trúc
- `ui/index.html`: toàn bộ giao diện (1 file HTML/CSS/JS, chạy trong WebView2).
- `src-tauri/src/`: Rust. `desktop.rs` (WorkerW, đa màn hình, hiện taskbar), `media.rs` (Windows Media), `audio.rs` (loopback + FFT), `net.rs` (search, lyrics, dịch), `accounts.rs` (đăng nhập YouTube/SoundCloud/Spotify), `volume.rs` (âm lượng Windows), `settings.rs`, `main.rs`.
- `GUIDE.txt`: hướng dẫn người dùng tiếng Anh (bản chính); `HUONG-DAN.txt`: bản tiếng Việt. Giữ cả hai cập nhật khi thêm tính năng.
- Mọi thứ hiện trên GitHub (README, CHANGELOG/release notes, GUIDE.txt, PRIVACY, CODE_SIGNING) viết bằng tiếng Anh. Giao diện app có tiếng Anh + tiếng Việt (`ui/i18n-vi.js`).

## Linux / macOS (bản thử, chưa chạy trên máy thật)
- Cùng một mã: các module chỉ có trên Windows (`desktop`, `deskmouse`, `media`, `audio`, `power`, `volume`, `boot`) có bản Linux/macOS cùng tên hàm trong `src-tauri/src/unix/` (chọn bằng `#[cfg_attr(not(windows), path = ...)]` trong `main.rs`). `unix/x11.rs` (Linux: màn hình, cửa sổ, chụp màn hình qua X11), `unix/mac.rs` (macOS: CoreGraphics), `unix/autostart.rs` (Mở khi đăng nhập). Code Windows giữ nguyên, chỉ thêm `#[cfg(windows)]`.
- Linux: skin là cửa sổ X11 kiểu DESKTOP (Wayland → chạy qua XWayland), nhạc app khác qua MPRIS (gio), âm lượng `wpctl`/`pactl`, viền sáng qua `parec`/`pw-record`. macOS: NSWindow ở level desktop, Spotify/Apple Music qua AppleScript, phím tắt mặc định ⇧⌘Space.
- Build Linux: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev` rồi `cargo build --release` → `target/release/wallplayer`. macOS: cần máy Mac, hoặc GitHub Actions: `.github/workflows/build-unix.yml` (push lên `main`, `custom`, `port/**`) build Linux + macOS + Windows, chạy thử app trên máy Linux và máy Mac thật của GitHub, quay video có chú thích (artifact `Test-video-Linux`, `Test-video-macOS`; script trong `.github/scripts/`). Không phát hành gì.
- Kiểm tra kiểu cho cả 3 hệ từ Linux: `cargo check --target aarch64-apple-darwin` (cần `CC_aarch64_apple_darwin=clang CFLAGS_aarch64_apple_darwin=--target=arm64-apple-macosx11.0`) và `cargo check --target x86_64-pc-windows-msvc`.

## Build (trên Windows)
```
cd src-tauri
cargo build --release
```
File ra: `src-tauri/target/release/wallplayer.exe`. Khi phát hành phải đi kèm `WebView2Loader.dll` cùng thư mục. Cần Rust (MSVC) và Microsoft Edge WebView2 Runtime.

## Phát hành
- Ghi release notes (tiếng Anh, kiểu "New / Changed / Fixed") dưới `## x.y.z` trong `CHANGELOG.md`, đổi version ở `src-tauri/Cargo.toml` và `src-tauri/tauri.conf.json` (phải giống nhau), push lên `main`.
- GitHub Actions (`.github/workflows/build.yml`) build trên Windows; Tấn thử bản build rồi bấm **Run workflow** (hoặc Claude đẩy tag `Vx.y.z` lên, khi Tấn bảo publish) để phát hành: workflow (bước ký SignPath chỉ chạy khi có `SIGNPATH_*`; SignPath Foundation đã từ chối dự án 11/2026 nên hiện bản phát hành không ký số) rồi tạo release `Vx.y.z` với 3 zip, mỗi hệ một file: `Wallplayer-Windows-x64.zip`, `Wallplayer-Linux-x64.zip`, `Wallplayer-macOS.zip` (thư mục `Wallplayer/` bên trong) và notes từ CHANGELOG (thêm bảng "Downloads"). Chỉ phát hành khi cả 3 bản build được. Bỏ tick "Publish a release" = chạy thử (dry run), không phát hành. Chi tiết: `docs/SIGNPATH-SETUP.md`.
- App tự cập nhật từ release mới nhất (`src-tauri/src/update.rs`), mỗi hệ tìm đúng zip của mình: tên 3 file zip và thư mục trong zip không được đổi. Tag dạng `V1.0.3`.

## Lưu ý
- **Mọi thay đổi phải làm đồng bộ cho cả 3 nền tảng: Windows, Linux, macOS** (giao diện chung `ui/index.html`; phần Rust của Windows ở `src-tauri/src/`, Linux/macOS ở `src-tauri/src/unix/`). Sửa một bên thì kiểm tra và sửa bên kia, cập nhật cả phần "Linux and macOS" trong GUIDE.txt / HUONG-DAN.txt, và chờ cả workflow "Linux and macOS (test builds)" chạy xong.
- Bản exe cũ được build chéo từ Linux và chưa từng chạy thử trên Windows thật: hãy build và kiểm tra lại trên máy Windows trước khi phát hành.
- Genius, YouTube InnerTube, dịch Google dùng API không chính thức, có thể đổi bất cứ lúc nào.
- Google lyrics không tự động hóa được (captcha), app chỉ mở trình duyệt.

- Chế độ mặc định = Wallpaper (WorkerW, như Wallpaper Engine). Chuột được chuyển bằng hook WH_MOUSE_LL (`src-tauri/src/deskmouse.rs`) → sự kiện `desk-mouse` → `deskMouse()` trong ui/index.html. Chưa chạy thử trên Windows thật: kiểm tra rê/bấm/kéo/cuộn, Win+D, và ô search (tự gọi `peek`).
