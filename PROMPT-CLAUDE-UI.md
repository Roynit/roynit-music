# Prompt cho Claude · Hoàn thiện giao diện Roynit Music

Hãy chỉnh giao diện Roynit Music trong repo Rust đang mở cho đủ chỉn chu để dùng nghe nhạc mỗi ngày trên Windows. Đọc `CLAUDE.md`, `tiendo.md` và `docs/UI-HANDOFF.md`, kiểm tra `git status --short`, rồi chạy bản hiện tại để xem bố cục thực tế. Giữ các thay đổi local, đặc biệt bản sửa “Nothing loaded”. Làm giao diện và chuyển động trên egui hiện tại.

## Ảnh minh họa

![Mockup Roynit Music với màn Listen now, cột Up next và thanh phát nổi Graphite + Iris](docs/design/roynit-music-reference.png)

Dùng ảnh để lấy tỷ lệ ba cột, mật độ danh sách, thứ bậc chữ và điểm nhấn ở thanh phát. Tên bài, thể loại và ảnh bìa trong hình là minh họa. Bản chạy dùng metadata và bìa thật. Nếu Claude không tự đọc ảnh nhúng, mở trực tiếp `docs/design/roynit-music-reference.png` trước khi sửa.

### Ảnh tham chiếu thêm cho nút và hàng đợi

![Riff gốc trên Windows: trang chủ, Now Playing và cụm điều khiển phát](test-output/riff-original-reference.png)

Ảnh trên chụp trực tiếp Riff đang cài trên máy. Mở `test-output/riff-original-reference.png` nếu Claude chưa đọc được ảnh. Riff gốc nhóm Shuffle và Repeat cạnh Previous/Play/Next, dùng icon đơn sắc, còn trạng thái bật có dấu riêng. Tham khảo cách giảm độ nổi của nút phụ. [Source Riff gốc](https://github.com/rootscripts/riff)

![Spotify Desktop: thanh phát gọn và cột hàng đợi](https://community.spotify.com/t5/image/serverpage/image-id/172609i957C17DC9E80D334/image-size/large?px=999&v=v2)

Ảnh trên từ [Spotify Community](https://community.spotify.com/t5/Desktop-Windows/Shuffle-mode-playing-the-same-songs-all-the-time/td-p/5280042/page/9). Xem thêm [video giao diện Spotify Desktop chính thức](https://storage.googleapis.com/pr-newsroom-wp/1/2023/06/Desktop-update-compressed.mp4) và [mô tả cách Spotify nhóm điều khiển Queue](https://newsroom.spotify.com/2025-05-07/experience-a-new-dimension-of-music-discovery-with-more-controls-and-enhanced-tools/). Học mật độ, thứ tự thao tác và cách báo trạng thái; dùng icon và tông Graphite + Iris riêng của Roynit.

**Ưu tiên nút Repeat/Shuffle trong lượt này.** Đổi hai nút dropdown lớn thành một cụm icon thấp, có tooltip và tên cho trình đọc màn hình. Repeat bấm để đổi vòng `Off → All → One`, trạng thái One có số 1. Shuffle mở menu gọn bốn lựa chọn của Roynit, dùng dấu chọn và nền nhẹ cho mục đang bật. Menu nằm sát nút, mỗi dòng cao khoảng 32–36 px logic, đủ chỗ cho nhãn và focus, không tô tím nguyên một hàng. Autoplay dùng công tắc cạnh nhãn “Autoplay”, mô tả dài để trong tooltip. Kiểm các trạng thái hover, focus, nhấn, bật và vô hiệu. Giữ toàn bộ hành vi và giá trị đã lưu.

## Chất Roynit

- **Graphite + Iris.** Nền graphite `#14151D`, mặt nổi `#1E202B`, chữ chính `#F2F1F7`, chữ phụ `#AAAEBE`, nút chính Iris `#AA98FF`. Dùng một bộ icon cùng nét và font đọc tốt tiếng Việt. Giữ độ tương phản rõ ở chế độ sáng lẫn tối.
- **Một điểm nhận diện.** Thanh phát nổi ở đáy có viền kính mỏng và quầng sáng nhẹ lấy màu từ bìa bài. Tập trung ánh sáng gần artwork. Lúc chưa phát chỉ dùng nền và viền nhẹ. Để phần thư viện và danh sách dùng bề mặt phẳng.
- **Nhạc đứng trước giao diện.** Cho bìa album, tên bài và nghệ sĩ tạo điểm nhìn. Tránh thẻ to giống nhau, khoảng trống không có tác dụng, gradient phủ rộng, nhãn “Rust preview” và các câu quảng cáo thừa trong màn nghe nhạc.

## Những chỗ cần sửa trước

1. **Listen now.** Thu thẻ Mix để thấy trọn ba thẻ ở cửa sổ desktop thông thường ngay cả khi Up next mở, và thấy vài dòng gợi ý mà chưa cần cuộn. Điều chỉnh kích thước theo phần nội dung còn lại. Ở cửa sổ hẹp, dùng hàng cuộn có dấu hiệu rõ hoặc giảm số cột. Bấm bìa hoặc tên để mở Mix, một nút Play trên ảnh, các thao tác phụ như lưu cho vào `…`. Bỏ “0 tracks” trên Home khi đã có Mix. Chỉ hiển thị thể loại nếu có dữ liệu thật.
2. **Điều hướng và Up next.** Sidebar thấp hơn để Downloads và Settings dễ với tới. Dùng ranh giới nhẹ giữa các vùng thay vì bọc mỗi mục vào một thẻ. Up next và Lyrics dùng chung bảng phải, đóng được, tự trở thành ngăn mở từ player khi thiếu chiều ngang. Giữ tên bài dài trên một dòng rút gọn và có cách xem tên đầy đủ. Làm rõ bài đang phát và bài kế tiếp.
3. **Thanh phát.** Bìa và tên bài bên trái. Previous, Play/Pause, Next và timeline có vị trí cố định. Âm lượng, Up next, Lyrics và More ở nhóm phải. Giữ nút dễ bấm, có nhãn truy cập và focus bàn phím. Cho thấy bài đang phát khác với bài đang chuẩn bị. Khi chưa có audio, thanh tua vẫn khóa.
4. **Search và Library.** Dùng cùng hàng bài hát và cách nhóm thao tác với Home. Giữ tìm kiếm hiện kết quả khi gõ, chip nguồn và bộ lọc có nhãn rõ. Ưu tiên tên bài, nghệ sĩ và thời lượng. Dùng trạng thái riêng cho đang tìm, không có kết quả và lỗi kết nối. Giữ hành vi chọn nhiều, lọc, xếp lại và tải offline.

## Quy cách chi tiết · Ít chữ, dễ thao tác

- **Chỉ hiện chữ giúp chọn hoặc thao tác.** Ưu tiên tên bài, nghệ sĩ, thời lượng và trạng thái. Bỏ slogan, “Rust preview”, nhãn lặp tiêu đề, đếm số bài không cần thiết và cụm giải thích dài. Không thay một đoạn văn bằng hàng loạt badge. Dùng nhãn đầy đủ cho trình đọc màn hình, tooltip cho chi tiết phụ.
- **Chữ:** tiêu đề trang 26–30, tiêu đề nhóm 18–20, tên bài 14–15, metadata 12–13 px logic. Tối đa ba độ đậm, font hỗ trợ tiếng Việt, chữ số thời lượng thẳng cột. Tên dài rút gọn một dòng và cho xem đầy đủ khi hover hoặc focus.
- **Bố cục:** lưới khoảng cách 4/8/12/16/24, lề 16–24, sidebar khoảng 180–200 và bảng phải khoảng 260–300 px logic khi đủ chỗ. Hàng bài cao 44–50, bìa 32–36. Hẹp thì thu bảng phải thành ngăn mở từ player. Giữ nút Play và ô tìm luôn dễ tới. Hạn chế viền và nền riêng quanh từng mục.
- **Nút:** dùng chung bộ icon, cỡ 16–20 và vùng bấm 32–36. Bình thường nền nhẹ hoặc trong suốt, hover làm nổi vừa đủ, focus có viền, active có Iris và dấu nhận biết ngoài màu, disabled đọc được. Chỉ một nút nổi nhất mỗi cụm. Nút ít quen như Import, Save và Retry dùng chữ ngắn.

| Thành phần | Thiết kế cụ thể |
|---|---|
| Ô tìm kiếm | Cao 36–40, kính lúp trái, placeholder **Search music**, nút xóa chỉ hiện khi có chữ, Ctrl+K hiện khi đủ chỗ. Nguồn nhạc nằm cạnh ô với tên và chevron nhỏ. Giữ tìm khi gõ và Enter gửi ngay. Nếu cần nút gửi riêng, dùng icon có tên truy cập “Search”. Spinner ở mép ô và không dời các nút. |
| Bộ lọc | Nhãn ngắn **Songs / Albums / Artists / Playlists**, active bằng một chỉ dấu. Không lặp tên nguồn trên từng bài khi toàn bộ danh sách cùng nguồn. Back giữ từ khóa và bộ lọc. |
| Hàng bài | Bìa → tên, nghệ sĩ → thời lượng → Like và More. Đổi số thứ tự thành Play khi hover/focus. Có cách bắt đầu chọn nhiều bằng bàn phím, và một thanh hành động chung khi đã có lựa chọn. |
| Play / Pause / Previous / Next | Play/Pause tròn 38–42, Iris. Previous/Next nhỏ hơn, không nền thường. Loading giữ kích thước và vị trí. Bấm lại khi đang tải giữ hành vi đã sửa cho lỗi “Nothing loaded”. |
| Tua / Âm lượng | Giữ thanh tiến độ và hai mốc thời gian ổn định. Handle nhỏ khi nghỉ, hiện rõ khi hover hoặc focus. Khóa tua khi chưa có audio. Icon loa bật/tắt tiếng, thanh âm lượng liền cạnh, giá trị qua tooltip. |
| Shuffle / Repeat / Autoplay | Shuffle icon mở menu Off / Classic / Balanced / Smart Shuffle. Repeat bấm xoay Off → All → One, thêm số 1 khi lặp một bài. Autoplay có công tắc và nhãn ngắn. Mỗi trạng thái có chỉ dấu ngoài màu và nhãn cho trình đọc màn hình. |
| Up next / Lyrics / More / Close | Dùng icon ở thanh phát, nút đang mở có dấu active. Trong bảng phải, một tiêu đề ngắn và nút đóng cùng hàng. Menu More dùng nhãn động từ, dấu chọn cho mục đang bật, tách mục xóa khỏi thao tác thường. |
| Thẻ Mix | Một Play trên bìa, mở Mix qua bìa hoặc tên, chuyển Save playlist vào menu ba chấm. Chữ trên bìa cần dải tối chuyển mượt, không chia sọc. |
| Nút quản lý | Giữ chữ khi là hành động chính: **Import**, **New playlist**, **Save**, **Retry**. Dùng menu cho hành động phụ; giữ xác nhận ở thao tác xóa đang có. |

**Cách viết trạng thái:** Một câu ngắn, tối đa một hành động hữu ích. Ví dụ **No results**, **Library is empty** + **Import**, **Couldn't load mixes** + **Retry**, **No lyrics**. Lỗi hiện gần nơi vừa thao tác; chi tiết kỹ thuật để trong mục xem thêm nếu cần. Không đưa log lên đầu màn hình.

## Phạm vi bàn giao cho mọi tab

Đợt này hoàn thiện toàn bộ giao diện, gồm các màn dưới đây, thanh phát và chế độ mini. Home chỉ là nơi chốt thành phần chung đầu tiên. Dùng chung kiểu hàng bài, nút icon, menu, input, focus và trạng thái phản hồi trong cả app. Mọi thao tác hiện có phải tìm được bằng bàn phím.

| Màn | Yêu cầu thiết kế và luồng cần thử |
|---|---|
| Listen now | Mix vừa khung, gợi ý nhìn thấy sớm. Thử mở Mix, phát và lưu playlist. Kiểm màn chưa có gu nghe, đang tạo Mix và nguồn gợi ý lỗi. |
| Search | Kết quả khi gõ, nhóm bài/album/nghệ sĩ/playlist, đổi nguồn dễ thấy. Kiểm truy vấn ngắn, không có kết quả và gõ nhanh khi mạng chậm. |
| Album / Artist / Playlist từ nguồn nhạc | Bìa, tiêu đề, metadata và nút phát có thứ bậc. Dùng hàng bài chung. Kiểm Back trở lại đúng kết quả và thao tác lưu bộ sưu tập hiện có. |
| Your library | Thanh lọc, sắp xếp và import gọn. Có cách rõ ràng để bắt đầu chọn nhiều bằng chuột lẫn bàn phím. Giữ thao tác hàng loạt và phân biệt bài local, online, đã tải. |
| Recently played | Dùng nhóm thời gian nếu dữ liệu có ngày giờ; giữ thứ tự gần nhất. Kiểm phát lại và tên bài dài. |
| Favorites | Danh sách dùng hàng bài chung, trạng thái Like nhìn rõ. Kiểm bỏ Like và trạng thái chưa có bài yêu thích. |
| Offline music | Cho thấy bài nào phát được offline, phân biệt file tải với file gốc. Kiểm phát khi mất mạng và thông báo nếu file mất. Giữ xác nhận ở các thao tác xóa hiện có. |
| Playlists | Bố cục danh sách playlist và nội dung playlist rõ ràng. Kiểm tạo, nhập link, đổi tên, thêm/xóa bài, sắp thứ tự và tải playlist. Hiển thị lỗi nhập ngay gần chỗ nhập. |
| Up next / Queue | Phân biệt bài đang phát với bài tiếp theo. Cùng một cụm Shuffle/Repeat/Autoplay ở trang đầy đủ và bảng phải. Kiểm xếp lại, Previous và trạng thái hàng đợi trống. |
| Lyrics | Dòng đang hát rõ, giữ dòng dài dễ đọc. Nút cuộn theo bài, dịch và chỉnh lời phải tìm được. Kiểm bài có timestamp, lời tĩnh, bài chưa có lời và click dòng để tua. |
| Downloads | Nhóm tác vụ đang chờ, đang chạy, xong, lỗi và đã hủy. Chỉ dùng phần trăm khi backend báo được. Kiểm hủy tác vụ và xem nguyên nhân lỗi. |
| Settings | Chia thành các nhóm Phát nhạc, Giao diện, Tài khoản và nguồn, Mạng, Dữ liệu và cập nhật. Dùng nhãn ngắn, phần giải thích sát thiết lập liên quan. Kiểm đổi thiết bị, theme, proxy, đăng nhập và cập nhật. |
| Mini player / thanh phát | Bố trí điều khiển vừa khung, trạng thái chưa phát, đang tải, đang phát và pause rõ. Kiểm tua, âm lượng, keyboard và đóng/mở chế độ mini. |

## Chuyển động và kiểm tra

Phản hồi Play ngay khi bấm. Chuyển bìa và màu khoảng 180 ms, bảng phụ khoảng 160–220 ms. Dùng chuyển động để giải thích thứ gì vừa mở, đổi hoặc được thêm vào. Giảm chuyển động thì cập nhật trực tiếp. Tránh animation lặp ở mọi thẻ và tránh hiệu ứng nặng khi cuộn hoặc phát nhạc.

Giữ bộ phát FFmpeg + rodio, thư viện hiện tại, Discover/Mix, nguồn Spotify/YouTube Music, kiểm đúng bản thu Spotify, Autoplay, Smart Shuffle, Windows SMTC và cơ chế cập nhật. Audio Studio nằm ngoài phạm vi. Ảnh này là tài liệu thiết kế, không dùng làm nền hoặc làm ảnh thay cho các thành phần tương tác.

Dùng **Listen now và player** để chốt thành phần chung, rồi áp chúng cho mọi màn trong bảng trước khi bàn giao. Kiểm ở cửa sổ 1120×700 và ít nhất một cửa sổ rộng hơn, với Up next bật và tắt. Chụp ảnh từng tab chính, bảng Up next/Lyrics và mini player, có ít nhất một ảnh lúc đang phát và một ảnh menu Repeat/Shuffle. Thử bàn phím, scaling Windows, giảm chuyển động, bản ghi thiếu artwork, chuyển bài, tua và Play khi chưa có audio. Báo theo từng màn đã kiểm, kèm ảnh hoặc đường dẫn; ghi rõ luồng chưa kiểm được và lý do. Chưa phát hành khi chưa có yêu cầu.
