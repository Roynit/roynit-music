# Prompt cho Claude · Làm lại Settings và rà toàn bộ Roynit Music

Đọc `PROMPT-CLAUDE-UI.md` để nắm thiết kế Graphite + Iris và phạm vi từng tab. Làm tiếp trên source Rust hiện tại, ưu tiên Settings. Dùng ảnh thật bên dưới để hiểu cách app nghe nhạc sắp nhóm và điều khiển. Roynit dùng thiết kế riêng, giữ thanh phát kính và hệ icon của mình.

## Ảnh chụp hai app trên Windows

![Riff gốc, Settings với nhóm bên trái và điều khiển căn phải](test-output/ui-reference-tabs/riff-02-settings.png)

![Spotify Desktop, Settings với các tiêu đề nhóm và hàng đồng nhất](test-output/ui-reference-tabs/spotify-09-settings.png)

Mở trực tiếp các PNG trong `test-output/ui-reference-tabs/` nếu Claude không đọc ảnh nhúng. Ảnh để tham khảo local, có thể chứa tên tài khoản, thư viện và bài nghe; giữ nguyên trong thư mục bị Git bỏ qua.

| App | Bộ ảnh tham chiếu |
|---|---|
| Riff, giao diện phát | `riff-01-home.png`, `riff-13-liked.png`, `riff-14-queue.png`, `riff-15-lyrics.png`, `riff-16-search.png` |
| Riff, Settings | `riff-02-settings.png` (Giao diện), `03-theme`, `04-banner`, `05-songs`, `06-integrations`, `07-storage`, `08-advanced`, `09-about`, `10-custom-themes`, `11-plugins`, `12-account` (cùng tiền tố `riff-` và đuôi `.png`) |
| Spotify, giao diện phát | `spotify-01-home.png`, `02-browse`, `03-liked`, `04-playlist`, `05-artist`, `06-queue`, `07-lyrics`, `08-profile-menu`, `10-search` (cùng tiền tố `spotify-` và đuôi `.png`) |
| Spotify, Settings | `spotify-09-settings.png` và `spotify-settings-scroll-1.png` đến `spotify-settings-scroll-5.png` |

## Sửa Settings theo hàng, không dàn thành một dãy nút

- Chọn **một bố cục nhất quán**: điều hướng Settings ở trái, nội dung một nhóm ở giữa, thanh phát giữ dưới. Có ô `Search settings` để lọc theo tên và từ đồng nghĩa nếu triển khai được trên dữ liệu thật. Thu Up next khi mở Settings để dành chiều ngang cho hàng thiết lập. Khi cửa sổ hẹp dùng danh sách nhóm hoặc một nút chọn nhóm; nhãn ở trên và điều khiển xuống hàng nếu không đủ chỗ.
- Một hàng chỉ giải quyết **một thiết lập**. Trái là tên ngắn, phải là giá trị, switch, slider hoặc một hành động. Cùng mép trái và mép phải cho các hàng. Dùng khoảng cách 12–16 px logic và đường phân cách nhẹ theo nhóm. Không bọc mọi hàng trong thẻ dày. Mô tả tối đa một câu ngắn ở những thiết lập thực sự cần giải thích. Đơn vị đặt cạnh giá trị.
- Nhóm **Playback**: Output một hàng, chọn thiết bị và báo lỗi ngay dưới nếu đổi không được. Khi cần nút Apply vì hành vi hiện có, chỉ hiện khi giá trị đã đổi. Refresh là nút icon có nhãn truy cập. Sleep timer một hàng có thời gian hoặc đếm ngược và một nút Start/Cancel theo trạng thái. **Autoplay**, **Crossfade** `0 s`, **Normalize volume**, **Media keys**, **Play in tray** mỗi mục một hàng. Không ghép ba tính năng trong một hàng.
- Nhóm **Downloads & storage**: **Download format** một dropdown, bỏ nhãn “Audio format” lặp; **Cache limit** dùng giá trị theo MB/GB và lưu khi hợp lệ hoặc có nút Save khi thay đổi; **Cache used** hiển thị số hiện tại cùng hành động Clear cache. Đưa giới hạn và việc giữ audio đang phát vào trợ giúp, tách hành động dọn dẹp khỏi thao tác thường.
- Nhóm **Appearance**: Theme, Accent, Text size, Reduce motion, Visualizer. Hiện giá trị hiện tại. Preview nhỏ nếu có dữ liệu thật. Nhóm **Accounts**: mỗi dịch vụ có một hàng trạng thái, Connect/Disconnect hoặc menu phụ; tùy chọn OAuth/cookie nâng cao nằm bên dưới, tránh đưa chi tiết kỹ thuật ra đầu. Không ghi bí mật vào ảnh hay log.
- Nhóm **Network**: Proxy có switch hoặc trạng thái, nút Test chỉ khi có cấu hình; lỗi gần trường nhập. Nhóm **Extensions**: Theme/Plugin bản native có Import, Enable và trạng thái ngắn gọn. Nhóm **Data & updates**: Backup/Restore và version/update theo cụm rõ. Giữ quyền, xác nhận, xử lý lỗi, các thiết lập đang lưu và API hiện có.

## Rà nốt các màn khác trước khi bàn giao

Dùng trực tiếp bảng từng tab và từng nút trong `PROMPT-CLAUDE-UI.md`. Từ Riff học cách nhóm điều khiển và giữ nổi bật artwork; từ Spotify học cách căn hàng Settings và thưa chữ. Với Settings, Up next, Search, Playlist và Lyrics, chụp cùng trạng thái tại cửa sổ 1120×700 và một khung rộng hơn. Kiểm focus bàn phím, menu khi ở sát mép cửa sổ, text dài có dấu, light/dark và giảm chuyển động. Tiêu chí đạt là không lặp nhãn, không mất nút, không có hàng xô cột khi thu cửa sổ, có thông báo khi lưu thất bại và các hành vi phát nhạc giữ nguyên. Không publish.
