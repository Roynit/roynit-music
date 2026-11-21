# Bàn giao Roynit Music

## Bắt đầu ở đây

- Thư mục source hiện tại là thư mục chứa file này, `C:\Users\HaiDang\Documents\ChatGPT\Roynit\roynit-music`. Mở cả thư mục để sửa, vì file này là bản đồ source và tiến độ.
- GitHub là `https://github.com/Roynit/roynit-music`. Commit nền `f464cd4` thuộc bản 0.1.2. **Các thay đổi phát nhanh, Mix và tìm khi gõ vẫn nằm trong working tree local**, gồm hai file mới `src/mixes.rs` và `src/youtube_stream.rs`. Clone GitHub lúc này sẽ thiếu các thay đổi đó. Chạy `git status --short` trước khi bắt đầu. Giữ các thay đổi hiện có.
- Bản thử vừa build nằm ở `test-output/mixes-preview/Roynit Music.exe`. Bản chính thức tải từ `https://roynit.dangz.org/?app=roynit-music` vẫn là 0.1.2; số version trong Cargo.toml cũng vẫn là 0.1.2.
- Thư mục `../riff` là Riff gốc để đọc và so sánh. Chỉ phát hành source Roynit Rust. Giữ giấy phép và thông báo upstream bắt buộc. Tên tác giả/commit dùng **dang**.

## Bản đồ source

| File | Việc phụ trách |
|---|---|
| `src/desktop.rs` | Giao diện egui, Listen now, Mix, tìm khi gõ, trạng thái phát |
| `src/lib.rs`, `src/jobs.rs` | Engine, giao thức JSON, hàng đợi tác vụ, hủy tác vụ và áp kết quả |
| `src/mixes.rs` | Chọn bài làm gốc từ lịch sử/Like, tạo Mix từ radio thật, lọc trùng gợi ý |
| `src/youtube.rs`, `src/youtube_stream.rs` | Search/Home/Radio YouTube Music và lấy link audio trực tiếp cho video đã biết |
| `src/provider.rs`, `src/spotify_public.rs`, `src/spotify.rs` | Nguồn nhạc, cache link, Spotify công khai và ghép đúng bản thu |
| `src/player.rs`, `src/system_media.rs` | FFmpeg → rodio, chuẩn bị PCM bài tiếp theo và Windows SMTC |
| `src/model.rs`, `src/store.rs` | Thư viện, lịch sử, playlist, hàng đợi và lưu dữ liệu |
| `src/updates.rs`, `scripts/package.ps1`, `scripts/publish-cloudflare.py` | Cập nhật, đóng gói và phát hành |

## Trạng thái đang chạy

- **Phát nhạc.** Với YouTube/YouTube Music công khai, thử hai đầu mối player song song, xác minh link audio HTTPS trước khi dùng, rồi rơi về yt-dlp nếu cần. Trường hợp có cookie, proxy hoặc browser config vẫn theo đường cũ. Nếu luồng trực tiếp lỗi lúc phát, lần thử lại chuyển sang đường dự phòng. Spotify vẫn phải ghép tiêu đề, nghệ sĩ, loại bản thu và thời lượng. Giữ các kiểm tra này khi tối ưu.
- **Độ trễ đã đo.** Backend Riff gốc lấy link hai bài mẫu trong 302–674 ms. Trên một bài YouTube, Roynit từ 4,9 s xuống 0,73 s ở lần phát đầu; bài kế tiếp có PCM sẵn mất khoảng 39 ms. Đây là số đo mẫu, thời gian có thể tăng với Spotify hoặc yt-dlp. `examples/latency_audit.rs` có phép đo tới PCM đầu tiên.
- **Chuẩn bị khi chọn bài.** Hàng đầu đang thấy hoặc hàng đang rê chuột có một bộ đệm PCM riêng, song song với một bộ đệm cho bài tiếp theo. Kiểm tra trùng URL, headers, proxy và hiệu ứng trước khi dùng lại. Hai bộ đệm đều giới hạn và tự hết hạn. Lượt đo mới trên một bài YouTube ghi nhận 87 ms khi đã chuẩn bị, 108 ms khi chỉ có link và 1,58 s khi chưa chuẩn bị.
- **Mix và gợi ý.** Tối đa 3 Mix, mỗi Mix tối đa 24 bài. Bài nghe gần đây, nghe nhiều và Like giúp chọn bài gốc; thứ tự trong Mix lấy từ radio YouTube Music. Home có gợi ý xen kẽ giữa các Mix, nút phát, xem bài và lưu playlist. Đây là cá nhân hóa theo dữ liệu Roynit, khác với trang Dành cho bạn trong tài khoản YouTube.
- **Cá nhân hóa YouTube.** Sau đăng nhập Google/YouTube đọc-only, Roynit lấy mẫu bài Like và playlist do tài khoản tạo, lọc video thuộc danh mục Music, rồi dùng làm bài gốc cho radio. Phiên cookie YouTube Music đã xác thực có thể bổ sung playlist đã lưu và tối đa ba Mix do YouTube trả về trên Home. Có thể xem nguồn tín hiệu ở Listen now và playlist tài khoản trong Settings. Việc gợi ý không tự nhập bài vào thư viện. Kết quả mix làm mới khi kết nối, ngắt hoặc thay phiên. Cần kiểm bằng tài khoản thật để xác nhận mức bao phủ và khả năng trả Mix theo vùng/tài khoản.
- **Autoplay.** Hàng đợi thông thường có thể tự nối bài liên quan từ radio YouTube Music khi còn tối đa ba bài. Bật mặc định, tắt trong Up next hoặc Settings. Không nối khi đang dùng Repeat, Shuffle hoặc radio riêng. Giữ thứ tự hàng đợi hiện tại, bỏ bài trùng và bỏ kết quả khi người nghe thay hàng đợi. Kiểm thử trực tiếp đã nối 24 bài và phát bài gợi ý đầu tiên.
- **Shuffle.** Menu dưới player có Off, Classic, Balanced và Smart Shuffle. Balanced ưu tiên đổi nghệ sĩ và tránh chọn lại vị trí đã phát nếu còn bài chưa phát. Smart giữ quy tắc đó và có thể chèn một bài radio sau ba bài khi đã có gợi ý, hoặc khi hết bài trong hàng đợi. Nút Previous dùng lịch sử của hàng đợi. Đây là bản native của luồng chính Better Shuffle, chưa có mở rộng kết quả tìm kiếm và hồ sơ bỏ qua/nghe đủ lâu của plugin gốc.
- **Tìm khi gõ.** Từ hai ký tự, chờ dừng gõ 300 ms rồi chạy search theo nguồn và loại nội dung đã chọn. Bỏ kết quả cũ theo generation, hủy job cũ khi search mới bắt đầu. Enter và nút Search vẫn gửi ngay.
- **Đã kiểm.** Clippy qua. Test YouTube Music thật tạo 2 Mix, 12 gợi ý không trùng và lưu playlist 24 bài. Cửa sổ Windows hiển thị 3 Mix từ thư viện hiện tại, và truy vấn đang gõ đã tự hiện kết quả. Những bản build này chưa được phát hành.

## Build và kiểm tra

Chạy trong thư mục repo, dùng Rust 1.100.0 theo CI. FFmpeg/FFprobe cần có trên PATH hoặc cấu hình qua biến môi trường; yt-dlp có thể cài từ Settings.

```powershell
cargo fmt --all -- --check
cargo clippy --locked --features desktop --all-targets -- -D warnings
cargo build --release --locked --features desktop --bins --example shuffle_queue
cargo test --locked --features desktop --all-targets
cargo test --locked --features desktop --test youtube -- --include-ignored --test-threads=1
cargo run --release --locked --example latency_audit
```

Có thể dùng `$env:CARGO_TARGET_DIR = (Resolve-Path ../riff/rust/target).Path` trên máy hiện tại để dùng cache build chung. Dùng `--data-dir <thư_mục_test>` khi cần kiểm dữ liệu thử. Dữ liệu thật nằm riêng dưới `%LOCALAPPDATA%\rootscripts\RiffRust\data`. Không đưa thư viện người dùng, cookie, khóa ký, `.wrangler/`, `dist/`, `target/` hoặc `test-output/` vào bản bàn giao công khai.

## Việc còn lại

Đã thử trình phát Chromium của Windows trong `examples/chromium_audit.rs`. Chạy `cargo run --release --locked --features chromium-audit --example chromium_audit` để đo cùng URL, không tính thời gian lấy link. Trên ba lượt cùng bài, FFmpeg/rodio ghi nhận PCM ở 88–89 ms, còn WebView2 báo `playing` ở 160–205 ms sau khi khởi tạo. WebView2 khởi tạo thêm 334–470 ms và bản chạy hoàn toàn ẩn từng bị timeout. Hai số dùng mốc phát khác nhau và chỉ đo trên một máy. Chưa tích hợp WebView2 vào desktop, để kết quả đo không ảnh hưởng bản người dùng.

1. Chạy lại bộ kiểm thử đầy đủ trên snapshot local cuối cùng. Kiểm tra Mix phát liên tục và tìm khi gõ nhanh khi mạng chậm hoặc đổi nguồn. Đo thêm bài Spotify và YouTube chưa có cache trước khi kết luận ngang Riff.
2. Cải thiện độ sát gu nếu cần bằng tín hiệu nghe đủ lâu, bỏ qua sớm và tùy chọn “ít bài kiểu này hơn”. Bản này đang dựa vào lượt phát và Like.
3. Hoàn thiện UI/UX và animation sau. Giữ trạng thái loading, lỗi, kết quả thật và luồng thao tác bàn phím. Audio Studio đã bỏ theo yêu cầu.
4. Kiểm tài khoản nhạc thật và tương tác với RoynitEye. Plugin JS/CSS gốc cần chuyển sang cấu trúc native. Xem `docs/EXTENSIONS.md`.
5. Khi phát hành, tăng version mới, kiểm SHA-256 và chạy thử cập nhật từ bản đã cài. Xem `docs/CLOUDFLARE.md`, `docs/DEVELOPMENT.md` và `docs/SIGNING.md`. Bản công khai hiện chưa ký chứng chỉ Windows. Không thay file của version đã phát hành.
