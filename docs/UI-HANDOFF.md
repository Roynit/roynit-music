# Bàn giao giao diện Roynit Music

Đọc `tiendo.md` để tìm đúng snapshot local. Giao diện mẫu ở `src/desktop.rs` dùng egui. Audio Studio đã loại khỏi phạm vi.

## Hướng thẩm mỹ

Lấy cảm giác kính tối của RoynitEye làm điểm chung trong hệ Roynit. Chọn nền **Graphite** `#14151D`, mặt nổi `#1E202B`, chữ `#F2F1F7`, chữ phụ `#AAAEBE` và điểm nhấn **Iris** `#AA98FF`. Dùng aqua `#78DDDA` thật nhẹ ở quầng sáng. Giữ danh sách nhạc trên nền đặc dễ đọc; dành kính trong và viền sáng cho thanh phát và bảng đang nghe. Dùng Geist hoặc Segoe UI có dấu tiếng Việt, với số thời gian có độ rộng cố định.

Chi tiết nhận diện là **thanh phát nổi lấy màu từ bìa bài nhạc**. Thu màu bìa thành một quầng nhỏ dưới lớp kính, giữ màu nút Play là Iris để luôn dễ nhận ra. Đổi bài thì quầng sáng chuyển cùng bìa. Khi chưa có bìa, dùng màu Iris tĩnh. Không cần làm hiệu ứng kính cho từng hàng và không đưa mascot của RoynitEye vào màn hình chính.

- **Desktop rộng:** thanh điều hướng gọn bên trái, tìm kiếm trên đầu, nội dung nhạc ở giữa, bảng Up next hoặc Lyrics bên phải có thể đóng. Thanh phát luôn thấy. Khi cửa sổ hẹp, chuyển bảng phải thành ngăn mở từ thanh phát.
- **Listen now:** đưa Mix cá nhân ra trước bằng bìa nhạc thật và nút Play. Phần gợi ý bên dưới dùng hàng gọn như thư viện. Để artwork làm điểm nhìn và dành chỗ cho tên bài, nghệ sĩ, nguồn.
- **Now Playing:** khi mở rộng, đưa bìa lên lớn và cho chọn Lyrics hoặc Up next. Lời hiện tại rõ hơn các dòng còn lại; nút tua, phát và âm lượng không nhảy vị trí.
- **Chuyển động:** nút phản hồi ngay, panel mở khoảng 160–220 ms, artwork chuyển khoảng 180 ms. Giảm chuyển động thì đổi trạng thái trực tiếp. Chuẩn bị artwork/hiệu ứng ngoài luồng âm thanh; tránh blur toàn cửa sổ mỗi khung hình.

Làm trước **Listen now + thanh phát**, rồi dùng chung thành phần cho Search, Library, Lyrics và Settings. Kiểm ảnh ở cả cửa sổ nhỏ lẫn lớn, sau đó kiểm bấm Play, gõ tìm, tua và chuyển bài khi có tải nền.

## Hành vi cần giữ

- Listen now hiển thị Mix theo lịch sử và Like, gợi ý liên quan, Home và Charts từ YouTube Music. Mỗi Mix có hành động phát, mở danh sách và lưu playlist. Cần thể hiện trạng thái đang tạo, chưa đủ lịch sử và lỗi nguồn.
- Search hiển thị kết quả ngay khi ngừng gõ. Giữ điều hướng bằng bàn phím, loại kết quả, nguồn đang chọn và khả năng bỏ phản hồi từ truy vấn cũ.
- Player phân biệt bài đang phát với bài đang tải. Giữ thông tin bản thu được ghép cho Spotify, timeline, seek, repeat, shuffle, nút Autoplay, lyrics và Windows media controls. Trạng thái tiếp nối bài chỉ xuất hiện khi hàng đợi thực sự nhận bài.
- Library, Playlist, Queue và Downloads cần bố cục rõ cho chọn nhiều, thứ tự, loại nguồn, trạng thái tác vụ và nút hủy. Giữ đường dẫn dữ liệu cũ để nâng cấp không mất thư viện.
- Settings nhóm nguồn nhạc, tài khoản, proxy, output, thông tin theme/plugin và cập nhật. Dùng điều khiển bật tắt cho chuyển động giảm, tránh giấu trạng thái lỗi thật.

Animation ưu tiên phản hồi nút bấm tức thì, giữ kích thước artwork và dòng nội dung ổn định khi đang tải. Kiểm light/dark, font lớn, DPI và cửa sổ nhỏ. Giữ toàn bộ thao tác chính dùng được khi giảm chuyển động.
