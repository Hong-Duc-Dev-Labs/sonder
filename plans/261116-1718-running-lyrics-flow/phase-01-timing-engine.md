---
phase: 1
title: "Phase 1: Algorithmic Timing Engine"
status: completed
priority: P1
effort: "2h"
dependencies: []
---

# Phase 1: Algorithmic Timing Engine

## Goal
Tạo bộ công cụ phân rã và ước lượng thời gian chạy từng từ (`spreadWords` & `prepareTimedLines`) trong `app/src/lib/lyrics.ts` kèm bộ unit test toàn diện trong `app/src/lib/lyrics.test.ts`.

## Context & Key Insights
Hầu hết các file `.lrc` chỉ có mốc thời gian cấp dòng (`[01:23.45]`). Thuật toán `SpreadWords` từ `egoist/mygo-music` giải quyết bài toán này mà không cần thay đổi cấu trúc dữ liệu ở backend Rust hay server:
- Phân bổ 85% thời lượng dòng cho ca từ, 15% thời lượng còn lại dành cho khoảng ngắt thở trước dòng kế tiếp.
- Tự động nhận diện chữ CJK (Trung, Nhật, Hàn) tách theo từng chữ cái (`rune`), còn tiếng Anh/Việt/Latinh tách theo khoảng trắng thành các từ.
- Trọng số thời gian phân bổ tỉ lệ thuận với số lượng ký tự của từng từ.

## Files to Create / Modify
- Modify: [app/src/lib/lyrics.ts](file:///D:/Dev/Sonder/app/src/lib/lyrics.ts)
- Modify: [app/src/lib/lyrics.test.ts](file:///D:/Dev/Sonder/app/src/lib/lyrics.test.ts)

## Tasks & Steps

1. **Định nghĩa kiểu dữ liệu (Types) trong `app/src/lib/lyrics.ts`**:
   - `TimedWord`: `{ text: string; startMs: number; endMs: number; }`
   - `TimedLine`: `LyricsLine & { endMs: number; words: TimedWord[]; }`

2. **Cài đặt hàm `spreadWords(text: string, startMs: number, endMs: number): TimedWord[]`**:
   - Regex kiểm tra CJK: `/\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}|\p{Script=Hangul}/u`.
   - Thuật toán gom cụm từ:
     - Gặp khoảng trắng: ngắt từ.
     - Gặp ký tự CJK: ngắt từ trước đó và đưa ký tự CJK thành 1 từ độc lập.
     - Ký tự thông thường (Latinh, dấu thanh tiếng Việt): gom tiếp vào từ hiện tại.
   - Tính tổng số ký tự thực tế (không tính khoảng trắng thừa).
   - Xác định `span = (endMs - startMs) * 0.85`.
   - Phân bổ: `wordDuration = span * (charCount / totalChars)`.
   - Gán `startMs` và `endMs` cho từng từ nối tiếp nhau.

3. **Cài đặt hàm `prepareTimedLines(lines: LyricsLine[]): TimedLine[]`**:
   - Với mỗi dòng `i`:
     - `endMs = lines[i+1] ? Math.min(lines[i].startMs + 6000, lines[i+1].startMs) : lines[i].startMs + 6000`.
     - Gọi `spreadWords(lines[i].text, lines[i].startMs, endMs)`.
   - Trả về mảng `TimedLine[]` đã được tính toán sẵn.

4. **Bổ sung Unit Tests trong `app/src/lib/lyrics.test.ts`**:
   - Test phân tách câu tiếng Anh: `"Hello world from Sonder"`.
   - Test phân tách câu tiếng Việt có dấu: `"Tôi thấy hoa vàng trên cỏ xanh"`.
   - Test phân tách câu CJK: `"春はあけぼの"` hoặc `"夜曲"`.
   - Test tỉ lệ phân bổ thời lượng 85% và thứ tự thời gian liên tục (`word[i].endMs === word[i+1].startMs`).
   - Test các trường hợp biên: dòng rỗng, dòng chỉ có khoảng trắng, dòng chỉ có 1 từ duy nhất, `endMs <= startMs`.

## Verification
- Chạy lệnh test: `pnpm --dir app test`
- Đảm bảo tất cả 11+ test suites đều pass không có lỗi.
