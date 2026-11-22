# Wallplayer — Backlog cải thiện (so với iBaila)

Xem thêm: [FEATURE-IDEAS.md](FEATURE-IDEAS.md) — tính năng cân nhắc từ research thị trường.

Nguồn: [iBaila](https://aestheticmac.com/ibaila) · [What's new](https://aestheticmac.com/ibaila/whats-new) (đọc ngày 20/11/2026)

iBaila cùng ý tưởng với Wallplayer: viền sáng theo nhạc, player kính cover flow trên desktop, lyrics đồng bộ, MV làm hình nền. Mac + Windows, 9,99 USD mua một lần, bán thêm gói giao diện player và gói hiệu ứng lyrics. 32 bản cập nhật từ 25/9 đến 17/11/2026.

## Wallplayer đang hơn

- Miễn phí, mã nguồn mở
- Lyrics dịch song ngữ 10 ngôn ngữ (có tiếng Việt), iBaila không có
- 13 hiệu ứng lyrics miễn phí (iBaila 8, thêm 28 phải mua)
- Chỉnh đồng hồ kiểu lock screen

## Ưu tiên 1 — Chạy ổn trên Windows thật

Các lỗi iBaila từng gặp trên PC, dùng làm checklist test:

- [ ] Chạy chung Wallpaper Engine: player chớp tắt hoặc bị đè
- [ ] Win+D / dọn desktop làm mất viền sáng
- [ ] Hình nền phủ kín mép màn hình; màn 4K báo đúng độ phân giải
- [ ] Spotify cài từ Microsoft Store vẫn phát được
- [ ] Tên user Windows có dấu tiếng Việt vẫn phát được
- [ ] Tai nghe gaming / SteelSeries Sonar: viền sáng vẫn nhảy
- [ ] Trình duyệt không bị kẹt "luôn trên cùng" sau khi app mở bài
- [ ] Một tiến trình phụ bị lỗi không làm cả app tắt

## Ưu tiên 2 — Nhẹ máy

- [x] Ngừng vẽ hẳn khi dừng nhạc
- [x] Tự giảm hiệu ứng khi chạy pin hoặc bật Battery Saver
- [x] Tự tạm dừng khi có game/phim toàn màn hình
- [x] Chế độ tiết kiệm (Optimized mode) trong Settings

## Ưu tiên 3 — Cài đặt và cập nhật

- [x] ~~File cài đặt thay cho zip~~ (giữ bản portable, tự cập nhật thay cho file cài)
- [x] Tự cập nhật
- [ ] Ký số file exe để SmartScreen không cảnh báo (SignPath Foundation đã từ chối vì dự án chưa đủ người dùng: nộp lại sau, hoặc mua chứng chỉ Certum Open Source; workflow vẫn giữ bước ký SignPath, chỉ chạy khi có `SIGNPATH_*`)

## Ưu tiên 4 — Tính năng còn thiếu

- [x] Queue: xem bài tiếp theo, sắp xếp, xóa
- [x] Lyrics: chỉnh lệch giờ bằng menu chuột phải và nhớ lại
- [x] Lyrics: nốt nhạc đếm ngược ở đoạn không lời
- [x] Lyrics: chọn phiên bản lyrics khác, tìm lyrics bằng tay
- [x] Nhạc trên máy: kéo thả thư mục vào playlist
- [x] Viền sáng: thêm hiệu ứng (họ 22, mình 5), xem trước khi chọn
- [x] Viền sáng: chỉ sáng cạnh dưới / cạnh trên / các góc
- [x] Viền sáng: bật tắt riêng từng màn hình
- [x] Icon khay đổi thành ảnh bìa, có vòng tròn đập theo nhịp
- [x] Nút Like ngay trên player
- [x] Chọn MV khác của cùng ca sĩ
- [x] 2 nguồn cùng phát thì hỏi muốn nghe nguồn nào
- [x] Giao diện tiếng Việt + tiếng Anh

## Để sau (khó)

- [ ] Sticker ảnh tách nền
- [ ] Hiệu ứng âm thanh: Nightcore, Slowed + Reverb, EQ
- [ ] AI canh lyrics cho bài chưa có lời
- [ ] Thêm kiểu player

## Học cách giới thiệu

- Release notes: tiêu đề ngắn mỗi bản, từng dòng "New:" / "Fixed:", viết bằng lời người dùng, cảm ơn người báo lỗi bằng tên
- Ra bản nhỏ, thường xuyên
- README thêm FAQ: có dùng micro không, xin quyền gì, tốn RAM và pin không
