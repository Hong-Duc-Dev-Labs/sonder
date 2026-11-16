---
phase: 3
title: "Phase 3: Stage Lyric Visualizer with Flow Style"
status: completed
priority: P1
effort: "3h"
dependencies: ["1", "2"]
---

# Phase 3: Stage Lyric Visualizer with Flow Style

## Goal
Xây dựng sân khấu ca từ toàn cảnh [LyricStage.svelte](file:///D:/Dev/Sonder/app/src/components/LyricStage.svelte) chỉ áp dụng riêng phong cách **Flow** trứ danh từ Folia-major / mygo-music (với vật lý lò xo nảy `springStep`, glow ánh sáng theo màu bìa đĩa, drift nhẹ nhàng, 3 chấm thở interlude) và tích hợp vào [NowPlayingView.svelte](file:///D:/Dev/Sonder/app/src/components/NowPlayingView.svelte).

## Context & Key Insights
- Người dùng yêu cầu: **"chỉ làm stage với flow"** -> Bỏ qua phong cách Steps (Partita) và Tilt (倾诉) để tập trung làm thật mượt mà và tinh gọn kiểu **Flow** cổ điển.
- Cơ chế của phong cách **Flow**:
  - Mỗi dòng có một hạt giống ngẫu nhiên xác định (`lineRandom(seed, offset)`) dựa trên thời điểm `line.startMs`. Nhờ đó, dòng hát luôn có độ nghiêng, độ lệch và tỷ lệ nảy tự nhiên độc nhất nhưng hoàn toàn nhất quán khi tua tới/lui.
  - Khi một từ được cất lên, nó phóng to và nảy vào vị trí với công thức lò xo tắt dần (`springStep(t)`).
  - Phía sau chữ có một lớp bóng phát sáng (`glow`) mờ ảo mang màu sắc chủ đạo của bìa album (`accent color`).
  - Khi từ đã được hát xong, nó từ từ ổn định kích thước và trôi nhẹ (`drift`).
  - Dòng trước biến mất mượt mà với đường cong `easeOutCubic` khi dòng mới xuất hiện.

## Files to Create / Modify
- Create: [app/src/lib/stageMotion.ts](file:///D:/Dev/Sonder/app/src/lib/stageMotion.ts)
- Create: [app/src/components/LyricStage.svelte](file:///D:/Dev/Sonder/app/src/components/LyricStage.svelte)
- Modify: [app/src/components/NowPlayingView.svelte](file:///D:/Dev/Sonder/app/src/components/NowPlayingView.svelte)
- Modify: [app/src/lib/locales/en.ts](file:///D:/Dev/Sonder/app/src/lib/locales/en.ts)
- Modify: [app/src/lib/locales/vi.ts](file:///D:/Dev/Sonder/app/src/lib/locales/vi.ts)

## Tasks & Steps

1. **Tạo module toán học chuyển động `app/src/lib/stageMotion.ts`**:
   - `springStep(t: number): number`:
     - Nếu `t <= 0` trả về 0.
     - `e = Math.exp(-10 * t)`
     - Trả về `1 - e * (Math.cos(10 * t) + Math.sin(10 * t))` (độ cứng 200, độ cản 20).
   - `lineRandom(seed: number, offset: number): number`:
     - `x = Math.sin(seed + offset) * 10000`
     - Trả về `x - Math.floor(x)` (tạo số ngẫu nhiên giả từ 0..1 có tính tất định).
   - `easeOutCubic(t: number): number`: `1 - Math.pow(1 - Math.min(1, Math.max(0, t)), 3)`.
   - `clamp01(v: number): number`: `Math.min(1, Math.max(0, v))`.
   - `lerp(a: number, b: number, t: number): number`: `a + (b - a) * t`.

2. **Xây dựng component `app/src/components/LyricStage.svelte`**:
   - Props / State: Nhận danh sách `timedLines`, `currentMs`, `accentColor` (trích xuất từ bìa album hoặc chủ đề hiện tại).
   - Xác định dòng hiện tại (`cur`) và dòng trước (`cur - 1`).
   - Sử dụng `onFrame` cập nhật tọa độ chuyển động của từng từ trong dòng:
     - Tạo `wordShapes` từ `lineRandom`: vị trí `x`, `y` lệch nhẹ ±18px, góc xoay `rotate` ±5°, độ co giãn `scale` 1.1..1.3, độ trôi `drift`.
     - Trạng thái chờ: thu nhỏ (`scale: 0.6`), lệch vị trí, mờ ảo (`opacity: 0`).
     - Trạng thái đang hát: nảy vào vị trí với `springStep(since)`, bung nở ánh hào quang rực rỡ (`glow = clamp01(since / (0.3 * singing))`).
     - Trạng thái đã hát: từ từ hạ nhẹ kích thước với `easeOutCubic`, trôi nhẹ (`drift`).
   - Hỗ trợ đoạn nghỉ (Interlude): Khi khoảng cách giữa các câu > 4s, hiển thị 3 chấm thở nhịp nhàng ở trung tâm sân khấu.
   - Hiển thị xem trước câu kế tiếp (`next line`) nhỏ hơn ở cạnh dưới cùng.

3. **Tích hợp vào `NowPlayingView.svelte`**:
   - Mở rộng kiểu dữ liệu `coverStyle` trong settings: `"vinyl" | "square" | "stage"`.
   - Cập nhật hàm `toggleStyle()` để quay vòng 3 chế độ:
     - `vinyl` (Đĩa than) -> `square` (Bìa vuông) -> `stage` (Sân khấu Flow) -> `vinyl`.
   - Khi ở chế độ `"stage"`, render `<LyricStage />` thay thế cho `TurntableStage` / `Cover` ở vùng thị giác chính (bên trái).
   - Cập nhật icon trên thanh công cụ: hiển thị biểu tượng tương ứng khi đang ở chế độ sân khấu lời.

4. **Bản địa hóa ngôn ngữ (`i18n`)**:
   - Thêm nhãn `"np.showStage": "Sân khấu lời bài hát"` / `"np.showStage": "Lyric Stage"` trong `en.ts` và `vi.ts`.

5. **Trợ năng & Performance**:
   - Hỗ trợ `reducedMotion()`: Tắt hoàn toàn độ nảy lò xo và độ nghiêng xoay, chuyển thành hiệu ứng hiển thị chuyển mờ thanh lịch.
   - Tối ưu hóa GPU: Sử dụng `transform: translate3d(...)` và `will-change: transform, opacity`.

## Verification
- Chạy kiểm tra kiểu: `pnpm --dir app check`
- Chạy kiểm tra test: `pnpm --dir app test`
- Chạy bản build thử nghiệm: `pnpm --dir app build`
- Kiểm tra tính mượt mà khi phát nhạc và tua bài hát (chuyển dòng ngay lập tức mà không bị giật lag).
