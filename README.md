# Sonder

[Tiếng Việt](README.md) | [English](README.en.md)

Trình phát nhạc lossless cục bộ: lõi Rust, giao diện Tauri, phát bit-perfect PCM và DSD qua chế độ độc quyền (exclusive-mode) trên Windows, macOS và Linux, tích hợp chuỗi DSP chuyên nghiệp tương tự Roon (headroom → SRC → convolution → PEQ → crossfeed → levelling → volume → dither, cùng tính năng chuyển đổi PCM→DSD).

## Định dạng hỗ trợ

- FLAC, ALAC (M4A), WAV, AIFF, DSF và DFF, hỗ trợ lên đến 32-bit/384 kHz và DSD256.
- Chế độ phát độc quyền (Exclusive mode) qua WASAPI (Windows), CoreAudio hog mode (macOS) và ALSA `hw:` (Linux).
- Phát DSD qua giao thức DoP (DSD over PCM), native DSD trực tiếp khi driver hỗ trợ (ALSA trên Linux), hoặc chuyển đổi (downsample) sang PCM.
- Chuyển đổi PCM → DSD64/128/256 bằng bộ điều chế sigma-delta single-bit (SDM) bậc 5 hoặc bậc 7, truyền tải qua DoP hoặc native DSD tùy theo hỗ trợ của thiết bị DAC.

Khi tắt toàn bộ các tính năng DSP và tắt âm lượng phần mềm (software volume), các byte dữ liệu ghi trực tiếp vào thiết bị chính là các sample đã giải mã nguyên bản (`crates/sonder-verify` chứng minh độc lập cho từng định dạng; `sonder-cli --verify-hash` xác thực kiểm tra mã băm trên DAC thực tế).

## Yêu cầu môi trường

- Rust 1.98 (xem `rust-toolchain.toml`), Node 22 + pnpm 12, và các điều kiện tiên quyết của Tauri 2 tương ứng với hệ điều hành của bạn ([tauri.app](https://tauri.app/start/prerequisites/)).
- Trên Linux cần cài đặt thêm thư viện ALSA (`libasound2-dev`) và WebKitGTK.
- DAC ngoài chỉ cần thiết khi cần kiểm thử chế độ phát độc quyền; bộ test tích hợp hoàn toàn có thể chạy trên backend ảo (null backend).

## Biên dịch và khởi chạy

```sh
# Môi trường phát triển (Vite + Tauri, hot reload)
cd app && pnpm install && pnpm tauri dev

# Đóng gói bộ cài đặt release cho hệ điều hành hiện tại
cd app && pnpm tauri build
```

Bộ cài đặt sau khi đóng gói sẽ nằm trong thư mục `target/release/bundle/`. Phần lõi Rust có thể hoạt động độc lập mà không cần giao diện đồ họa (UI):

```sh
cargo run -p sonder-cli -- devices
cargo run -p sonder-cli -- play ~/Music/album
cargo run -p sonder-cli -- play ~/Music/album --pcm-to-dsd dsd128 --sdm-order 5
cargo run -p sonder-cli -- play ~/Music/album --verify-hash
```

## Các cài đặt đáng chú ý

- **Thiết bị đầu ra (Output device)** — Chế độ độc quyền (exclusive mode) được yêu cầu cho mọi thiết bị; danh sách hiển thị rõ các định dạng và tần số lấy mẫu mà từng thiết bị tiếp nhận.
- **Âm lượng (Volume)** — Tắt âm lượng phần mềm (Off) đồng nghĩa với bit-perfect. Việc bật âm lượng phần mềm sẽ chèn thêm một bước xử lý DSP vào đường truyền và đánh dấu đường tín hiệu là "không bit-perfect".
- **Phát DSD (DSD playback)** — Tùy chọn DoP, native DSD (trên Linux) hoặc luôn chuyển sang PCM; tần số khi chuyển DSD→PCM là 176.4 kHz hoặc 352.8 kHz.
- **PCM sang DSD (PCM to DSD)** — Tắt (Off), DSD64, DSD128 hoặc DSD256, với bộ điều chế bậc 5 hoặc bậc 7. Mức tín hiệu 0 dBFS PCM tương ứng với độ điều chế 50% (theo quy ước chuẩn SACD), do đó luồng DSD sẽ thấp hơn 6 dB so với toàn thang đo. Trên CPU laptop đời 2022, toàn bộ đường xử lý PCM→DSD256 (bao gồm resample cộng bộ điều chế bậc 7) tiêu tốn khoảng 0.4–0.7 nhân CPU cho mỗi luồng stereo, DSD128 chỉ tốn khoảng một nửa; nên chọn DSD128 nếu DSD256 bị giật/drop out trên các máy có cấu hình yếu hơn (xem thêm `docs/verification.md`).
- **Tăng tần số lấy mẫu (Upsampling)** — Tắt, đẩy lên mức tối đa của thiết bị DAC, theo cùng họ tần số gốc (family rate 44.1k/48k), hoặc một tần số cố định, sử dụng bộ lọc do Sonder thiết kế (tuyến tính - linear hoặc pha tối thiểu - minimum phase) hoặc nạp từ thư mục bảng lọc Roon bạn sở hữu.
- **Thư mục bộ lọc Roon (Roon filter folder)** — Vào `Settings → Upsampling → Filter source: Roon` và chọn thư mục chứa các bảng `polyphase_*` trích xuất từ Roon. Sonder sẽ đọc trực tiếp từ đó lúc chạy và không bao giờ sao chép vào mã nguồn của mình. Nếu thiếu bảng lọc cho một tỷ lệ cụ thể, hệ thống sẽ tự động chuyển về bộ lọc nội bộ của Sonder và hiển thị rõ nguồn đang dùng trên Signal Path. Các bảng lọc này không được phân phối kèm theo Sonder (xem `.gitignore` và `scripts/check-forbidden-files.sh`).
- **Tích chập (Convolution)** — Vào `Settings → Convolution → Choose…` để nạp tệp phản hồi xung WAV (định dạng số nguyên 16/24/32-bit hoặc số thực 32-bit float, độ dài tối đa 2 giây): hỗ trợ một kênh cho tất cả các kênh đầu ra hoặc mỗi kênh đầu ra một kênh xung riêng. Bước này chạy sau upsampling và được resample lại một lần cho mỗi tần số luồng; khi bộ chuyển đổi đưa tín hiệu vào bộ điều chế DSD trên 384 kHz, tích chập sẽ chạy ở tần số nguồn thay thế. Giao diện Settings hiển thị độ lợi đỉnh (peak gain) của phản hồi; giá trị dương cho biết cần hạ preamp EQ hoặc headroom xuống bao nhiêu dB. Nếu không thể đọc tệp, tính năng convolution sẽ tự tắt kèm thông báo và quá trình phát nhạc vẫn tiếp tục.
- **Parametric EQ** — Hỗ trợ tối đa 32 dải băng tần (peak, low/high shelf, low/high pass, band pass, notch) đặt sau preamp, lưu dưới dạng preset có đặt tên. **Import…** cho phép đọc tệp cấu hình parametric của Equalizer APO — ví dụ như tệp `ParametricEQ.txt` do AutoEQ cung cấp cho từng mẫu tai nghe — và áp dụng thành preset mang tên tệp (bỏ hậu tố ` ParametricEQ`); nếu tên đã tồn tại sẽ hỏi trước khi ghi đè. Các dòng cấu hình dành cho giai đoạn APO khác hoặc giá trị bị Sonder giới hạn sẽ được liệt kê trong một cảnh báo, tệp không phải parametric EQ sẽ bị từ chối mà không làm thay đổi thiết lập hiện tại. **Export…** xuất EQ hiện tại dưới định dạng tương tự (dòng `Preamp:` tiếp theo là các dòng `Filter N:`) cho Equalizer APO; nhập lại tệp đó sẽ cho thông số EQ y hệt. Các loại filter được nhận diện: `PK`/`PEQ`, `LSC`/`LS`, `HSC`/`HS`, `LP`/`LPQ`, `HP`/`HPQ`, `BP`, `NO`; `BW Oct` được quy đổi sang hệ số Q, còn shelf hoặc pass không có Q sẽ mặc định dùng 0.707. **Find headphone…** tìm kiếm trực tiếp trên kho dữ liệu dự án [AutoEQ](https://github.com/jaakkopasanen/AutoEq) (khoảng 8.800 cấu hình bù trừ tai nghe) theo tên tai nghe và liệt kê kết quả kèm người đo đạc (ưu tiên cấu hình từ oratory1990 và crinacle); khi chọn một mẫu, tệp `ParametricEQ.txt` tương ứng sẽ được tải về và import như trên. Quá trình tìm kiếm thực hiện ngay trên máy: Sonder tải chỉ mục AutoEQ từ GitHub trong lần tìm kiếm đầu tiên, lưu cache 1 tuần (và dùng bản offline nếu mất mạng), chỉ tải về đúng tệp cấu hình bạn đã chọn.
- **Crossfeed** — Tính năng crossfeed bs2b cho tai nghe đối với các bản thu stereo, đặt sau EQ, cung cấp sẵn các preset của libbs2b: Default (700 Hz, 4.5 dB), Chu Moy (700 Hz, 6 dB) và Jan Meier (650 Hz, 9.5 dB). Âm thanh trung tâm giữ nguyên cường độ ở dải trầm và giảm nhẹ 1–1.75 dB ở dải trên tần số cắt, đúng theo hành vi chuẩn của bs2b.
- **Crossfade** — Hòa trộn mượt mà cân bằng công suất (equal-power fade từ 1–10 giây, mặc định 4 giây) từ bài hiện tại sang bài tiếp theo trong hàng đợi, cũng như khi bấm chuyển bài (Next) lúc đang phát. Tự động bỏ qua giữa các bài liên tiếp trong cùng một album, khi có sự thay đổi định dạng ngõ ra, với các tệp DSD và khi đang bật chế độ lặp một bài (repeat-one); đường dẫn tín hiệu (signal path) sẽ nêu rõ lý do ("crossfade skipped: same album"). Khi tắt crossfade, quá trình chuyển bài là ghép liền mạch gapless chính xác từng byte.
- **Lặp lại (Repeat)** — Nút cạnh nút Next chuyển đổi xoay vòng: Tắt → Tất cả (all) → Một bài (one). Chế độ repeat-one phát lại bài hát liền mạch không ngắt quãng và nút Next thủ công vẫn chuyển tiếp bài; repeat-all sẽ quay vòng từ bài cuối về bài đầu không gián đoạn; khi tắt lặp lại sẽ giữ nguyên bài đã giải mã trước đó.
- **Trình phát thu nhỏ (Mini player)** — Trong `Settings → Mini player` (mặc định bật): khi đóng cửa sổ chính lúc đang có bài hát được nạp, cửa sổ chính sẽ ẩn đi và một cửa sổ nhỏ ghim trên cùng (always-on-top) sẽ xuất hiện ở góc dưới cùng bên phải màn hình với ảnh bìa album, đĩa than xoay cùng các nút điều khiển lùi bài / phát-tạm dừng / chuyển bài khi di chuột qua. Bạn có thể kéo thả cửa sổ này đi bất cứ đâu; nhấp đúp hoặc bấm nút mở rộng để khôi phục lại cửa sổ chính; nút đóng trên mini player sẽ thoát hoàn toàn ứng dụng Sonder. Khi tắt cài đặt này hoặc khi hàng đợi trống, đóng cửa sổ chính sẽ thoát ứng dụng như bình thường.
- **Cập nhật (Updates)** — Trong `Settings → Updates` hiển thị phiên bản hiện tại và kiểm tra bản phát hành mới trên GitHub Releases; tùy chọn **Check on launch** (mặc định bật) sẽ tự động kiểm tra mỗi khi khởi động ứng dụng và hiển thị thông báo khi có bản mới. Bấm **Install … and restart** sẽ tải bộ cài, xác minh chữ ký mã hóa minisign với public key tích hợp trong ứng dụng, tiến hành cài đặt và khởi động lại. Ứng dụng không bao giờ tự ý tải hay cài đặt nếu chưa có thao tác bấm xác nhận, và việc kiểm tra cập nhật không gửi bất kỳ dữ liệu thư viện nào.
- **Ngôn ngữ (Language)** — Trong `Settings → Appearance → Language`: hỗ trợ Tiếng Anh (English), Tiếng Việt, hoặc Theo hệ thống (System - ưu tiên ngôn ngữ đầu tiên của hệ điều hành mà Sonder hỗ trợ, mặc định là Tiếng Anh). Giao diện chuyển đổi ngôn ngữ tức thì; các thông điệp từ audio engine hoặc hệ điều hành (lỗi thiết bị, tên các giai đoạn trong signal path) được giữ nguyên tiếng Anh. Thư mục bản dịch nằm tại `app/src/lib/locales/`; tệp `vi.ts` được ép kiểu nghiêm ngặt theo `en.ts`, do đó bất kỳ khóa dịch nào bị thiếu hoặc dư thừa đều sẽ báo lỗi ngay khi chạy `pnpm check`.

Màn hình Now Playing hiển thị trực quan toàn bộ chuỗi đường truyền tín hiệu (signal path) theo từng giai đoạn (bấm vào biểu tượng cạnh dòng thông tin thiết bị DAC).

## Danh sách phát (Playlists)

Danh sách phát được lưu trữ trong cơ sở dữ liệu thư viện SQLite (phiên bản schema 4 trở lên; phiên bản cũ hơn sẽ được tự động sao lưu thành `library.db.bak-3` trước khi nâng cấp). Tạo danh sách phát tại trang Playlists hoặc từ menu ⋯ của bất kỳ bài hát nào → **Add to playlist** → **New playlist…**; trang chi tiết danh sách phát hỗ trợ phát, phát ngẫu nhiên, phát tiếp theo hoặc thêm toàn bộ vào hàng đợi; từng hàng có thể kéo thả sắp xếp bằng tay cầm grip (hoặc phím mũi tên Lên/Xuống trên grip) và xóa khỏi danh sách qua menu ⋯. Đổi tên và xóa danh sách phát nằm trong menu ⋯ của trang; thao tác xóa luôn yêu cầu xác nhận và tuyệt đối không bao giờ làm ảnh hưởng đến các tệp nhạc gốc.

Các mục trong danh sách phát lưu đường dẫn tệp tuyệt đối thay vì track id của thư viện, giúp danh sách không bị mất khi quét lại thư viện. Tệp bị di chuyển hoặc xóa sẽ hiển thị trạng thái "not in library" (không có trong thư viện) và sẽ tự phục hồi khi quá trình quét tìm thấy tệp tại đường dẫn đó.

Tính năng **Import M3U…** trên trang Playlists cho phép chuyển tệp `.m3u` hoặc `.m3u8` thành danh sách phát mới mang tên tệp. Hỗ trợ bảng mã UTF-8 (có hoặc không có BOM) và Latin-1, đường dẫn tuyệt đối cũng như tương đối (tương đối theo thư mục chứa tệp m3u, chấp nhận cả hai loại dấu gạch chéo `/` và `\`), các URL dạng `file://`, đồng thời tự động bỏ qua các dòng cú pháp `#EXTM3U` và `#EXTINF`. Các bài hát chưa có trong thư viện được giữ dưới dạng "not in library"; các luồng web stream sẽ được bỏ qua và thông báo sẽ tổng hợp số lượng của cả hai loại. Tính năng **Export M3U…** trong menu ⋯ của playlist xuất ra định dạng M3U8 mở rộng: UTF-8, dòng `#EXTINF:<giây>,<nghệ sĩ> - <tiêu đề>` đối với các bài có trong thư viện, cùng đường dẫn tuyệt đối cho từng mục (bao gồm cả các bài đang bị thiếu).

## Thống kê nghe nhạc (Listening stats)

Sonder chỉ ghi nhận một lượt nghe hợp lệ khi bài hát đã thực sự phát được ít nhất 30 giây, hoặc một nửa thời lượng bài nếu bài ngắn hơn 30 giây. Việc tua (seek) hoặc tạm dừng (pause) không được tính vào thời gian nghe thực tế, và bài hát bị bỏ qua (skip) trước mốc thời gian trên sẽ không được lưu. Trang **Stats** trên thanh bên (sidebar) tổng hợp dữ liệu theo các mốc 7 ngày, 30 ngày, 12 tháng qua hoặc toàn thời gian. Trang hiển thị tổng thời gian nghe, số lượt phát và số bài hát khác nhau, biểu đồ thời gian nghe (theo ngày, hoặc theo tháng nếu chọn mốc từ một năm trở lên), danh sách bài hát, nghệ sĩ và album nghe nhiều nhất, cùng lịch sử các bài vừa nghe gần đây; nhấp chuột vào mục bất kỳ để phát bài hoặc mở trang nghệ sĩ/album tương ứng. Dữ liệu lịch sử nằm trong cơ sở dữ liệu thư viện (schema phiên bản 6; bản v5 được sao lưu thành `library.db.bak-5`), đánh chỉ mục theo đường dẫn tệp để không bị mất khi quét lại, và tuyệt đối không bao giờ gửi ra ngoài máy tính. Nút **Clear history** ở cuối trang cho phép xóa toàn bộ lịch sử nghe sau khi xác nhận.

## Hẹn giờ ngủ (Sleep timer)

Nút biểu tượng mặt trăng trên thanh điều khiển cho phép hẹn giờ tắt nhạc sau 15, 30, 45, 60 hoặc 90 phút, hoặc tự động dừng khi kết thúc bài hát hiện tại, đồng thời hiển thị số phút còn lại. Bộ đếm thời gian chạy trực tiếp trong backend Rust, do đó vẫn hoạt động chuẩn xác ngay cả khi cửa sổ chính đã đóng và chỉ còn mini player hiển thị. Khi hết giờ hẹn, quá trình phát sẽ tạm dừng kèm thông báo trên màn hình. Tùy chọn "Hết bài này" (End of this track) sẽ dừng lại ngay khi bài tiếp theo chuẩn bị bắt đầu (do cơ chế gapless phát liên tục không có khoảng lặng giữa các bài), vì vậy có thể xuất hiện một phần nhỏ giây âm thanh của bài kế tiếp trước khi dừng hẳn.

## Chỉnh sửa siêu dữ liệu (Editing metadata)

Mở menu ⋯ của bất kỳ bài hát nào → **Edit metadata** sẽ hiển thị các thẻ tag được lưu trực tiếp trong tệp — tiêu đề (title), nghệ sĩ (artists), album, nghệ sĩ album (album artists), số thứ tự bài và đĩa, năm phát hành, thể loại (genre), ảnh bìa nhúng và lời bài hát (lyrics) — và bấm **Save** sẽ ghi trực tiếp các thay đổi vào tệp âm thanh đó, sau đó tự động tái đánh chỉ mục (re-index) bài hát ngay tại chỗ (album sẽ cập nhật theo tag mới). Các định dạng FLAC, ALAC (M4A), WAV và AIFF được ghi thông qua thư viện lofty; định dạng DSF và DFF được thay thế thẻ ID3v2 trong container, và thẻ tag nằm sau form của tệp DFF (do công cụ như sacd_extract tạo ra) cũng được ghi đè chính xác tại vị trí đó. Các thông tin ReplayGain, bình luận (comments) và các thẻ tag kỹ thuật khác được giữ nguyên vẹn; ảnh bìa và lời bài hát chỉ thay đổi khi bạn chủ động thao tác. Ảnh bìa mới nạp (JPEG hoặc PNG) sẽ trở thành ảnh bìa mặt trước (front cover) và giữ nguyên các ảnh phụ khác; thao tác gỡ ảnh bìa sẽ xóa toàn bộ các ảnh nhúng trong tệp. Lời bài hát được lưu trữ nguyên văn như đã nhập — văn bản thuần (plain text) hoặc lời đồng bộ LRC kèm mốc thời gian dạng `[mm:ss.xx]`, bảng hiển thị lời bài hát sau đó sẽ cuộn theo các mốc này. Khi có nhiều nghệ sĩ, nhập phân tách bằng dấu chấm phẩy (`;`) sẽ được lưu thành các giá trị riêng biệt, nhờ đó mỗi nghệ sĩ sẽ là một mục độc lập trong thư viện và trên các trình phát nhạc khác. Mọi thẻ tag mà tệp hỗ trợ đều được đồng bộ chỉnh sửa (ví dụ: thẻ RIFF INFO đi cùng ID3v2 của tệp WAV), do đó trường nào xóa trắng sẽ thực sự bị xóa sạch. Quá trình ghi tệp được thực hiện an toàn qua một tệp tạm bên cạnh và kế thừa nguyên vẹn quyền truy cập (permissions) của tệp gốc; nếu quá trình ghi thất bại, tệp gốc hoàn toàn không bị ảnh hưởng, và nếu tệp hoặc thư mục bị khóa chỉ đọc (read-only) sẽ có thông báo lỗi rõ ràng. Trình chỉnh sửa hiển thị chính xác những gì tệp đang có: số thứ tự bài do thư viện tự phỏng đoán từ tên tệp sẽ không được tự động đưa vào danh sách lưu. Việc sửa tag vẫn bảo lưu nguyên vẹn dữ liệu âm lượng (loudness) đã đo lường trước đó; bài hát nào bị đổi sang album mới sẽ nhận giá trị âm lượng của album đó trong đợt quét phân tích tiếp theo.

Mặc định không có bất kỳ thao tác nào trong trình chỉnh sửa tự ý kết nối mạng. Nút **Find info online** gửi tiêu đề, nghệ sĩ và album đã nhập đến đồng thời iTunes, Deezer và MusicBrainz, sau đó liệt kê các kết quả tìm thấy, ưu tiên hiển thị trước các bản ghi có thời lượng trùng khớp với bài hát; việc chọn một kết quả sẽ tự động điền các trường tương ứng và tải về ảnh bìa (chỉ tải trực tiếp từ máy chủ ảnh của các dịch vụ uy tín này và Cover Art Archive). Nút **Find lyrics online** tra cứu lời bài hát từ các dịch vụ tương tự như bảng lời nhạc (LRCLIB, BetterLyrics, LyricsPlus, Kugou) dựa trên thông tin đã nhập, ngay cả khi tính năng tải lời online đang tắt trong Settings, và nạp nội dung tìm được vào ô nhập lời. Trong cả hai trường hợp, không có bất kỳ dữ liệu nào được ghi vào đĩa cho đến khi bạn bấm **Save**.

## Kiểm tra chất lượng âm thanh lossless (Quality check)

Trang **Quality check** trên thanh bên cho phép kiểm tra độ chân thực của bất kỳ bài hát nào: chọn bài từ danh sách tìm kiếm trong thư viện, bấm **Check what's playing** (Kiểm tra bài đang phát), hoặc bấm **Choose a file…** để kiểm tra bất kỳ tệp âm thanh nào trên đĩa cứng (tệp nằm ngoài thư viện vẫn được phân tích nhưng kết quả sẽ không lưu vào cơ sở dữ liệu). Chọn menu ⋯ của bài hát → **Check quality**, hoặc bấm vào huy hiệu chất lượng của bài, cũng sẽ mở trang kiểm tra cho bài hát đó. Quá trình kiểm tra sẽ giải mã toàn bộ tệp và đưa ra kết luận phân loại xem đây có phải là tệp lossless thực thụ hay không: **Genuine lossless** (Lossless chuẩn xịn), **Suspect** (Đáng ngờ), **Transcoded from lossy** (Nén giả cầy từ MP3/AAC), **Fake hi-res (upsampled)** (Hi-res giả lập upsample), **Fake bit depth** (Độ sâu bit giả) hoặc **Inconclusive** (Chưa đủ căn cứ xác định do thời lượng quá ngắn, âm lượng quá nhỏ hoặc thiếu dải cao; không tính là đạt chuẩn), kèm theo tỷ lệ độ tin cậy (confidence). Báo cáo liệt kê chi tiết các bằng chứng kỹ thuật theo từng hàng:

- **Tần số cắt phổ, độ dốc và độ ổn định (Spectral cutoff, slope and stability)** — Các bộ mã hóa lossy MP3/AAC áp dụng bộ lọc thông thấp (low-pass) cố định ở ngưỡng 16–20.5 kHz, tạo ra một "mái trần" phẳng đứng và đột ngột trên toàn bộ phổ bài hát; trong khi đó các bản thu âm thực thụ có dải tần vượt trên 21.5 kHz hoặc suy giảm tự nhiên với đường biên trên biến thiên linh hoạt theo giai điệu bản nhạc.
- **Lỗ hổng quang phổ (Spectral holes) và stereo tần số cao (high-frequency stereo)** — Hiện tượng các dải tần cao bị rớt quãng ngắt quãng theo từng khoảnh khắc, và việc dải cao bị gộp về mono (joint stereo) là những dấu vân tay đặc trưng không thể che giấu của định dạng lossy.
- **Độ sâu bit thực tế (Real bit depth)** — Được đo đạc chính xác từ từng mẫu âm thanh: một tệp 24-bit nhưng byte thấp nhất luôn bằng 0 thực chất chỉ là tệp 16-bit được chèn thêm số 0 để giả dạng (padded 16-bit).
- **Nội dung Hi-res thực (Hi-res content)** — Một tệp có tần số lấy mẫu từ 88.2 kHz trở lên nhưng xuất hiện bức tường cắt đứng ở ngưỡng 20–24 kHz là dấu hiệu rõ ràng của việc upsample từ nguồn CD 44.1k/48k.
- **Nguồn gốc DSD (DSD source)** — Tệp DSD xuất hiện bức tường tần số giới hạn của chuẩn CD ẩn dưới lớp nhiễu của bộ điều chế sẽ được cảnh báo là đáng ngờ (suspect), không khẳng định là giả mạo (bởi vì các bản thu chuyển từ băng từ analog cổ cũng có thể tạo ra phổ tương tự).
- **Mã kiểm tra FLAC MD5** — Chứng minh dữ liệu sau khi giải mã trùng khớp hoàn toàn với dữ liệu lúc nén ban đầu; giá trị này không phản ánh chất lượng nguồn thu âm gốc.

Trang phân tích còn trực quan hóa biểu đồ phổ trung bình (trục kHz tuyến tính) kèm tần số cắt phát hiện được, các chỉ số định dạng đo lường thực tế, và bản đồ quang phổ theo thời gian (spectrogram). Bạn có thể kiểm tra hàng loạt cho cả một album (bấm nút biểu tượng phổ trên trang album), cho danh sách phát (menu ⋯) hoặc cho toàn bộ bài hát (trang Songs), xử lý lần lượt từng bài kèm thanh tiến trình và nút Cancel trên thanh bên; bài nào đã có kết quả cache hợp lệ sẽ không cần giải mã lại. Kết quả được lưu trữ trong cơ sở dữ liệu thư viện (schema phiên bản 5; cơ sở dữ liệu v4 được sao lưu thành `library.db.bak-4`), đánh chỉ mục theo đường dẫn tệp và tự động hủy bỏ khi tệp có thay đổi, đồng thời hiển thị dưới dạng huy hiệu nhỏ trên danh sách bài hát. Toàn bộ quá trình chạy hoàn toàn cục bộ trên máy, không tải dữ liệu lên mạng và không sửa đổi tệp nhạc. Thuật toán phân tích dựa trên ước lượng quang phổ: các ngưỡng phát hiện đã được tinh chỉnh qua các tín hiệu nhân tạo và mẫu thử từ nhiều album thực tế, tuy nhiên một số bộ mã hóa có bộ lọc low-pass đặc biệt vẫn có thể được nhận diện là chuẩn.

```sh
cargo run --release -p sonder-cli -- check ~/Music/album          # Xuất báo cáo văn bản cho từng tệp
cargo run --release -p sonder-cli -- check track.flac --json      # Xuất báo cáo chi tiết dưới dạng JSON
```

## Kiểm thử và xác thực

```sh
cargo test --workspace          # Chạy toàn bộ unit test + integration test, bao gồm cả acceptance suite
cargo clippy --workspace --all-targets -- -D warnings
cargo bench -p sonder-dsp --bench sdm
cargo bench -p sonder-dsp --bench convolver
bash scripts/check-forbidden-files.sh
pwsh tools/check_bundle.ps1     # Thêm tham số -Build để biên dịch installer trước khi kiểm tra
python tools/design_sdm.py      # Kiểm tra độ ổn định và định hình nhiễu (noise shaping) của modulator
```

Thư mục `crates/sonder-verify/tests/verify.rs` chứa bộ kiểm thử nghiệm thu (acceptance suite): đảm bảo phát âm thanh chính xác từng byte cho từng định dạng, kiểm tra cổng ngắt thiết bị cho cờ `--verify-hash`, khả năng triệt tiêu tần số ảo của bộ lọc resampler, đáp tuyến tần số PEQ so với công thức chuẩn của Robert Bristow-Johnson (RBJ), cơ chế dither, chu trình chuyển đổi DoP, chu trình chuyển đổi SDM và mô hình chuỗi đường dẫn tín hiệu (signal-path model). Tệp `docs/verification.md` ghi nhận nhật ký các bài kiểm tra đã chạy trên phần cứng thực tế và các hạng mục đang tiếp tục theo dõi.

## Quy trình phát hành phiên bản

Bộ tự động cập nhật trong ứng dụng (in-app updater) hoạt động dựa trên các bản phát hành: ứng dụng đọc tệp `latest.json` từ bản phát hành mới nhất được công bố trên GitHub (không phải bản nháp, không phải pre-release). Mỗi bộ cài đặt bắt buộc phải được ký bằng khóa updater chuyên dụng mà phần public key được khai báo tại `plugins.updater.pubkey` trong `app/src-tauri/tauri.conf.json`. Nếu làm mất private key này, các bản cài đặt hiện tại của người dùng sẽ không thể tự động cập nhật được nữa, do đó hãy sao lưu cẩn thận.

1. Nâng thông số `version` trong tệp `Cargo.toml` ở thư mục gốc (ứng dụng dùng chung version của workspace) và trong `app/package.json`, sau đó merge vào nhánh `main` và đẩy tag lên git (ví dụ `v0.2.0`).
2. Quy trình CI/CD `release` workflow sẽ kiểm tra tính trùng khớp giữa hai phiên bản, tự động biên dịch bộ cài cho Windows, macOS (Universal binary) và Linux, ký số với các secret `TAURI_SIGNING_PRIVATE_KEY` và `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, sau đó đính kèm các gói cài đặt cùng tệp `latest.json` vào một bản phát hành nháp (draft release). Trước khi bấm xuất bản bản nháp, hãy mở `latest.json` và kiểm tra để đảm bảo tệp đã liệt kê đủ tất cả các nền tảng (do các tiến trình build chạy song song có thể ghi đè lẫn nhau). Ghi chú phát hành (release notes) trong ứng dụng được sao chép vào `latest.json` ngay lúc build, vì vậy việc sửa văn bản mô tả trên bản nháp sau đó sẽ không làm thay đổi nội dung này.
3. Trong trường hợp đóng gói thủ công không qua workflow, hãy build trên từng hệ điều hành với biến môi trường chứa khóa ký số, sau đó tạo manifest; khi build trên hệ điều hành thứ hai, truyền manifest của hệ điều hành trước qua cờ `--merge` để tạo ra một tệp duy nhất chứa tất cả các nền tảng. Script sẽ in ra đường dẫn của từng bộ cài và chữ ký `.sig` tương ứng để đính kèm theo định dạng dòng `attach:`:

   ```sh
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/sonder-updater.key)"
   export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
   (cd app && pnpm tauri build)
   node scripts/updater-manifest.mjs v0.2.0 --notes notes.md > latest.json
   gh release create v0.2.0 latest.json <toàn bộ các đường dẫn attach:> --notes-file notes.md
   ```

Bộ cập nhật tải các tệp phát hành mà không cần thông tin đăng nhập, vì vậy tính năng này chỉ hoạt động khi các bản phát hành trên repository được để ở chế độ công khai (public).

## Cấu trúc Repository

```
crates/sonder-decode   Giải mã FLAC/ALAC/WAV/AIFF qua symphonia; đọc DSF/DFF; xử lý bit DSD
crates/sonder-output   Backend xuất âm thanh độc quyền (WASAPI/CoreAudio/ALSA), renderer, DoP marker, null backend cho test
crates/sonder-dsp      Lõi lọc FIR, bộ resampler, PEQ, chuẩn hóa âm lượng (levelling), volume, dither, DSD→PCM, SDM
crates/sonder-engine   Luồng phát âm thanh (player thread), hàng đợi gapless, thứ tự chuỗi DSP, đường tín hiệu (signal path), đóng gói DoP
crates/sonder-library  Cơ sở dữ liệu SQLite thư viện, trình quét đĩa, cache ảnh bìa, phân tích độ lớn âm thanh, playlist
crates/sonder-lyrics   Xử lý định dạng LRC/TTML và tải lời bài hát trực tuyến kèm bộ nhớ đệm trên đĩa
crates/sonder-metadata Tìm kiếm thông tin bài hát và ảnh bìa từ iTunes, Deezer và MusicBrainz
crates/sonder-autoeq   Cấu hình bù trừ tai nghe AutoEQ: tải chỉ mục, tìm kiếm offline cục bộ, tải tệp EQ
crates/sonder-quality  Kiểm tra chất lượng lossless: tần số cắt phổ, dấu vết lossy, độ sâu bit thực tế
crates/sonder-cli      Liệt kê thiết bị, bộ harness phát bit-perfect, quét thư viện, kiểm tra chất lượng qua CLI
crates/sonder-verify   Bộ kiểm thử nghiệm thu và xác thực chất lượng âm thanh (acceptance suite)
app                    Giao diện người dùng Svelte 5 (Runes) + vỏ ứng dụng Tauri 2
tools                  Công cụ tính toán bảng SDM, kiểm tra tính toàn vẹn của bundle cài đặt
plans                  Kế hoạch phát triển theo từng giai đoạn, báo cáo kỹ thuật và tài liệu bằng chứng
```
