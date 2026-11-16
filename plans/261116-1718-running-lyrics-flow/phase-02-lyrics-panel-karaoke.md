---
phase: 2
title: "Phase 2: Karaoke Running Glow & Interludes in LyricsPanel"
status: completed
priority: P1
effort: "2h"
dependencies: ["1"]
---

# Phase 2: Karaoke Running Glow & Interludes in LyricsPanel

## Goal
Nâng cấp giao diện cuộn lời bài hát [LyricsPanel.svelte](file:///D:/Dev/Sonder/app/src/components/LyricsPanel.svelte) từ việc chỉ sáng cả dòng tĩnh sang hiệu ứng ca từ chạy chữ từng từ phát sáng (Karaoke Running Glow) và hiển thị 3 chấm nhịp thở (`interlude`) khi dạo nhạc.

## Context & Key Insights
- Hiện tại, `LyricsPanel.svelte` kiểm tra `activeLine(lyrics.lines, playbackMs())`. Dòng đang hát được thêm class `.line.on` (chỉ tăng opacity và text-shadow toàn dòng).
- Bằng cách sử dụng `prepareTimedLines` từ Phase 1, với dòng `on`, ta render từng từ bọc trong thẻ `<span class="word">`.
- Khi kim nhạc chạy qua từng từ, từ đó sẽ chuyển trạng thái từ mờ (`0.38`) sang sáng rực rỡ (`1.0`) kèm ánh hào quang, tạo cảm giác chạy chữ tự nhiên như Apple Music / MyGo Music.
- Trong các đoạn intro hoặc đoạn dạo nhạc giữa bài dài hơn 4 giây, thêm 3 chấm nhịp thở nhấp nháy êm dịu để người nghe biết bài hát đang trong đoạn nghỉ.

## Files to Create / Modify
- Modify: [app/src/components/LyricsPanel.svelte](file:///D:/Dev/Sonder/app/src/components/LyricsPanel.svelte)
- Modify: [app/src/lib/locales/en.ts](file:///D:/Dev/Sonder/app/src/lib/locales/en.ts) (nếu cần nhãn dịch)
- Modify: [app/src/lib/locales/vi.ts](file:///D:/Dev/Sonder/app/src/lib/locales/vi.ts)

## Tasks & Steps

1. **Khởi tạo dữ liệu Timed Lines trong `LyricsPanel.svelte`**:
   - Import `prepareTimedLines`, `type TimedLine`, `type TimedWord` từ `../lib/lyrics`.
   - Tạo derived state: `let timedLines = $derived(lyrics?.synced ? prepareTimedLines(lyrics.lines) : null);`
   - Theo dõi mốc thời gian phát hiện tại `playbackMs()` thông qua `clock.svelte.ts` và `onFrame`.

2. **Cập nhật Render dòng đang hát (`line.on`)**:
   - Khi `i === active` và dòng có danh sách `words`:
     - Render danh sách các thẻ `<span class="word" class:lit={isLit} class:current={isCurrent}>`.
     - `isLit`: `currentMs >= word.startMs`.
     - `isCurrent`: `currentMs >= word.startMs && currentMs < word.endMs`.
   - Khi dòng không phải `active`: Render văn bản bình thường như cũ để tiết kiệm DOM node và tối ưu GPU.

3. **Thêm hiệu ứng CSS Karaoke & Ánh sáng phát quang**:
   - `.word`: `display: inline-block`, `transition: opacity 120ms ease-out, text-shadow 160ms ease-out`.
   - `.word:not(.lit)`: `opacity: var(--lyric-idle, 0.38)`.
   - `.word.lit`: `opacity: 1`, `text-shadow: 0 0 16px rgba(255, 255, 255, 0.35)`.
   - `.word.current`: có thể thêm scale nhẹ `scale(1.04)` để tạo độ nảy của chữ khi ca sĩ cất giọng.

4. **Hiển thị 3 chấm nhịp thở Interlude (Instrumental Breaks)**:
   - Nhận diện đoạn dạo:
     - Đoạn mở đầu: Trước dòng đầu tiên nếu `timedLines[0].startMs > 4000`.
     - Đoạn giữa: Giữa dòng `i` và `i+1` nếu `timedLines[i+1].startMs - timedLines[i].endMs > 4000`.
   - Khi nhạc đang ở đoạn dạo này: Hiển thị component 3 chấm tròn `.interlude-dots` với hiệu ứng thở nhịp nhàng `@keyframes breath { 0%, 100% { opacity: 0.3; transform: scale(0.9); } 50% { opacity: 0.9; transform: scale(1.15); } }`.

5. **Giữ tương tác cuộn và trợ năng**:
   - Giữ nguyên cơ chế `hold()` khi người dùng lăn chuột (`onwheel={hold}`).
   - Khi `reducedMotion()` bật: vô hiệu hóa animation thở của chấm và scale của chữ, chỉ dùng chuyển đổi opacity đơn giản.

## Verification
- Chạy kiểm tra kiểu: `pnpm --dir app check`
- Chạy kiểm tra unit test: `pnpm --dir app test`
- Đảm bảo giao diện cuộn mượt mà, bấm vào dòng bất kỳ vẫn seek chính xác tới mốc thời gian đó.
