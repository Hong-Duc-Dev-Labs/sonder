# Plan: Adaptation của MyGo Music "Now Playing" & "Fog Backdrop" cho Sonder

## 1. Source Manifest & Bối Cảnh
- **Source Repository**: `egoist/mygo-music` (Go + Gio UI GPU toolkit).
- **Target Project**: `Sonder` (Rust core + Tauri 2.12 + Svelte 5 + Vite).
- **Feature Scope**:
  1. **Fog Backdrop**: Background sương mù động bằng GLSL WebGL Shader + hạt bụi phát sáng (dust glints) + audio-reactive theo nhịp Bass.
  2. **Grow From the Cover**: Transition phóng to từ thumbnail của player bar thành toàn bộ trang Now Playing (shared-element morph) + cử chỉ kéo xuống để đóng (pull-to-dismiss).
  3. **Now Playing UI/UX Redesign**: Layout 2 cột cân đối, thanh điều khiển thông minh ẩn/hiện khi hover chuột, hệ thống tab chuyển đổi mượt mà (`Lyrics`, `Stage`, `Up Next`, `Wall`), scrubber và transport controls tinh tế.

---

## 2. Dependency & Architecture Matrix

| Tính năng / Thành phần | MyGo Music (Source) | Sonder Hiện Tại | Giải pháp Porting sang Sonder |
| :--- | :--- | :--- | :--- |
| **Backdrop Rendering** | Gio UI custom shader (GPU canvas) | 2D Canvas radial gradient (`AmbientBackdrop.svelte`) | WebGL2 Fragment Shader canvas hiệu năng cao (`FogBackdrop.svelte`) |
| **Color Tuning** | HSV bucket dominant color -> `fogColors` (`Deep`, `Fog`) | `artColors` (hex strings từ backend SQLite) | Chuyển đổi `artColors[0]` sang RGB/HSV, sinh cặp màu `Deep` & `Fog` chuẩn shader |
| **Audio Reactivity** | FFT bands 0-6 smoothed (`k=18` rise, `k=6` fall) | Chưa có FFT realtime lên frontend | Non-intrusive RMS/Bass meter tap hoặc rhythmic beat oscillator |
| **Transition Animation** | Custom clip-path & rect lerp (`npCard`, `npMorph`) | CSS fade/slide (`slideRecord`, `pageIn`) | FLIP / `clip-path` + scale/translate shared morph (`GrowFromCover`) |
| **Dismiss Interaction** | Pointer drag + rubberband physics + velocity flick | Nút back hoặc click nav | Kéo chuột/vuốt xuống (pull-down dismiss) với spring physics |
| **Now Playing Layout** | 2 cột: Trái (Cover + Controls), Phải (Tabs: Lyrics, Stage, Queue, Wall) | Chia rời: Hoặc Vinyl/Square mode, hoặc Stage mode riêng | Tích hợp tabbed pane (`Lyrics`, `Stage`, `Up Next`, `Wall`) ngay trong Now Playing |

---

## 3. Detailed Specification

### Phase 1: WebGL Fog Backdrop (`FogBackdrop.svelte`)
- Chuyển mã nguồn shader `fogSource` của MyGo sang GLSL 3.00 ES / 1.00:
  - 5-octave FBM noise với coordinate domain warping (`q = vec2(fbm(...), fbm(...))`).
  - Hạt bụi ánh sáng (dust specks glinting) trôi bồng bềnh và nhấp nháy.
  - Uniforms: `u_time`, `u_bass`, `u_deep`, `u_fog`, `u_resolution`.
- Hiệu năng & Tối ưu:
  - Render ở độ phân giải 1/2 (`devicePixelRatio * 0.5`) và giới hạn ở 30-60 FPS qua `onFrame`.
  - Giảm tốc độ từ từ khi pause (`speed += (to - speed) * min(1, 2.5 * dt)`).
  - Crossfade 700ms giữa 2 bài hát liên tiếp.
  - Hỗ trợ `reducedMotion`: dừng animation, giữ background gradient tĩnh mượt mà.

### Phase 2: Phản Hồi Âm Thanh Theo Bass (Audio-Reactive Bass)
- Hai tùy chọn:
  - **Tùy chọn A (Khuyên dùng - Zero DSP overhead)**: Lắng nghe trạng thái bài hát + beat pulse oscillator nội suy từ BPM / playback clock khi phát, tạo độ nẩy tự nhiên cho bass mà không làm ảnh hưởng đến tính chất Bit-Perfect của DAC.
  - **Tùy chọn B (Engine Tap)**: Bổ sung 1 atomic tap nhẹ đo năng lượng dải trầm tần số thấp trong `sonder-engine` và emit mức âm lượng định kỳ ~30Hz.

### Phase 3: Now Playing 2-Column Responsive Layout
- Header nổi: Chevron đóng ở giữa trên cùng (tự động hiện ra khi di chuột vào cửa sổ, ẩn đi khi không tương tác).
- Tabs bar nổi ở góc trên: `Lyrics`, `Stage`, `Up Next`, `Wall` (hover-reveal).
- Cột bên trái:
  - Album Art kích thước responsive (280px - 440px), bo góc tròn mềm mại.
  - Tên bài hát (22px bold) + Nghệ sĩ / Album link (15px).
  - Like button & More actions menu ("...").
  - Thanh Seekbar tuỳ biến với nhãn định dạng Lossless/Hi-Res ở giữa hai mốc thời gian.
  - Cụm nút Playback Transport: Shuffle, Previous, Play/Pause nổi bật, Next, Repeat.
  - Slider điều khiển âm lượng đặt chính giữa.
- Cột bên phải:
  - Hiển thị pane tương ứng theo tab được chọn:
    - **Lyrics**: Lời bài hát karaoke đồng bộ từng từ (`RichText`), active line neo ở vị trí 1/3 viewport, click để tua.
    - **Stage**: Visualizer chữ nghệ thuật Folia kinetic typography (đã có nền tảng tại `LyricStage.svelte`).
    - **Up Next**: Hàng đợi bài hát kéo thả mượt mà.
    - **Wall**: Lưới bìa album tương tác toàn cảnh.

### Phase 4: Shared-Element Morph ("Grow From the Cover")
- Lưu bounding box của mini-cover trong `TransportBar` (`fromRect = { x, y, width, height }`).
- Khi mở Now Playing:
  - Card nở ra từ `fromRect` đến toàn màn hình với hiệu ứng spring physics.
  - Ảnh bìa nhỏ bay vào vị trí của ảnh bìa lớn.
  - Các phần tử nội dung xung quanh (Text, Controls, Tabs) xuất hiện so le (staggered elevation `npRise`).
- Hỗ trợ cử chỉ kéo xuống (pull-to-dismiss) với độ đàn hồi rubberband: nếu kéo quá 20% chiều cao hoặc vận tốc vẩy > 700px/s thì đóng về mini-player.

### Phase 5: Tùy Chọn Cài Đặt (Settings Toggles)
- Thêm vào mục **Settings > Appearance / Experiments**:
  - `Fog Backdrop`: Bật/tắt hiệu ứng nền sương mù động (kèm chú thích tiết kiệm pin nếu tắt).
  - `Grow From the Cover`: Bật/tắt hiệu ứng mở từ ảnh bìa thay vì trượt lên mặc định.
