# Wallplayer — ghi chú cho Claude Code

Chủ dự án: Tấn (Senior UX/UI Product Designer, không biết code). **Luôn trả lời bằng tiếng Việt**, giải thích đơn giản, tự làm hết các bước (build, commit, push), chỉ hỏi lại khi thật sự cần. **Không tự ý phát hành (publish release): chỉ publish khi Tấn ra lệnh.**

## Dự án
App Windows (Tauri v2 + Rust) biến hình nền desktop thành trình phát nhạc kiểu "aesthetic desktop skin" (đồng hồ cao, cover flow kính, search nhiều nền tảng, playlist 3D, lyrics đồng bộ + phụ đề 2 ngôn ngữ, ambient light viền neon nhảy theo nhạc, chỉnh đồng hồ/lyrics kiểu lock screen iPhone với chữ kính).

## Cấu trúc
- `ui/index.html`: toàn bộ giao diện (1 file HTML/CSS/JS, chạy trong WebView2).
- `src-tauri/src/`: Rust. `desktop.rs` (WorkerW, đa màn hình, hiện taskbar), `media.rs` (Windows Media), `audio.rs` (loopback + FFT), `net.rs` (search, lyrics, dịch), `accounts.rs` (đăng nhập YouTube/SoundCloud/Spotify), `volume.rs` (âm lượng Windows), `settings.rs`, `main.rs`.
- `GUIDE.txt`: hướng dẫn người dùng tiếng Anh (bản chính); `HUONG-DAN.txt`: bản tiếng Việt. Giữ cả hai cập nhật khi thêm tính năng.
- Mọi thứ hiện trên GitHub (README, CHANGELOG/release notes, GUIDE.txt, PRIVACY, CODE_SIGNING) viết bằng tiếng Anh. Giao diện app có tiếng Anh + tiếng Việt (`ui/i18n-vi.js`).

## Build (trên Windows)
```
cd src-tauri
cargo build --release
```
File ra: `src-tauri/target/release/wallplayer.exe`. Khi phát hành phải đi kèm `WebView2Loader.dll` cùng thư mục. Cần Rust (MSVC) và Microsoft Edge WebView2 Runtime.

## Phát hành
- Ghi release notes (tiếng Anh, kiểu "New / Changed / Fixed") dưới `## x.y.z` trong `CHANGELOG.md`, đổi version ở `src-tauri/Cargo.toml` và `src-tauri/tauri.conf.json` (phải giống nhau), push lên `main`.
- GitHub Actions (`.github/workflows/build.yml`) build trên Windows; Tấn thử bản build rồi bấm **Run workflow** (hoặc Claude đẩy tag `Vx.y.z` lên, khi Tấn bảo publish) để phát hành: workflow (bước ký SignPath chỉ chạy khi có `SIGNPATH_*`; SignPath Foundation đã từ chối dự án 11/2026 nên hiện bản phát hành không ký số) rồi tạo release `Vx.y.z` với zip `Wallplayer-Windows-x64.zip` (thư mục `Wallplayer/` bên trong) và notes từ CHANGELOG. Chi tiết: `docs/SIGNPATH-SETUP.md`.
- App tự cập nhật từ release mới nhất (`src-tauri/src/update.rs`): tên asset và thư mục trong zip không được đổi. Tag dạng `V1.0.3`.

## Lưu ý
- Bản exe cũ được build chéo từ Linux và chưa từng chạy thử trên Windows thật: hãy build và kiểm tra lại trên máy Windows trước khi phát hành.
- Genius, YouTube InnerTube, dịch Google dùng API không chính thức, có thể đổi bất cứ lúc nào.
- Google lyrics không tự động hóa được (captcha), app chỉ mở trình duyệt.

- Chế độ mặc định = Wallpaper (WorkerW, như Wallpaper Engine). Chuột được chuyển bằng hook WH_MOUSE_LL (`src-tauri/src/deskmouse.rs`) → sự kiện `desk-mouse` → `deskMouse()` trong ui/index.html. Chưa chạy thử trên Windows thật: kiểm tra rê/bấm/kéo/cuộn, Win+D, và ô search (tự gọi `peek`).
