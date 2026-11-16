---
title: "Running Lyrics Effect & Folia Flow Stage"
description: "Port running lyrics effect from egoist/mygo-music into Sonder: word-by-word timing synthesis, karaoke running glow in lyrics panel, and cinematic Folia Flow lyric stage in Now Playing."
status: completed
priority: P1
effort: "7h"
branch: main
tags: [feature, frontend, lyrics, animation]
blockedBy: []
blocks: []
created: 2026-11-17
---

# Running Lyrics Effect & Folia Flow Stage

## Overview

Kế hoạch chuyển đổi và nâng cấp hiệu ứng chạy lời bài hát từ `egoist/mygo-music` (bộ diễn họa Folia) vào Sonder. Kế hoạch được phân bổ thành 3 pha thực thi độc lập, rõ ràng:
1. **a (Pha 1)**: Bộ tính toán thời gian chạy từng từ (`spreadWords` & `prepareTimedLines`) trong `lyrics.ts` kèm unit tests.
2. **b (Pha 2)**: Hiệu ứng Karaoke chạy chữ phát sáng từng từ và 3 chấm nhịp thở dạo nhạc trong `LyricsPanel.svelte`.
3. **c (Pha 3)**: Sân khấu ca từ toàn cảnh `LyricStage.svelte` với riêng phong cách **Flow** chuyển động lò xo tắt dần, hào quang ánh sáng và tích hợp vào `NowPlayingView.svelte`.

## Cross-Plan Dependencies

| Relationship | Plan | Status |
|---|---|---|
| Builds upon | `261112-1100-synced-lyrics` | completed |

## Phases

| # | Phase | Status | Effort |
|---|---|---|---|
| 1 | [Algorithmic Timing Engine](./phase-01-timing-engine.md) | Completed | 2h |
| 2 | [Karaoke Running Glow & Interludes](./phase-02-lyrics-panel-karaoke.md) | Completed | 2h |
| 3 | [Stage Lyric Visualizer with Flow Style](./phase-03-lyric-stage-flow.md) | Completed | 3h |

## Key Decisions

1. **Thuật toán tự chia mốc từ (`spreadWords`)**: Không cần thay đổi backend hay dịch vụ API; phân rã trên client với 85% thời lượng cho tiếng hát và 15% cho nhịp thở, hỗ trợ CJK và tiếng Việt.
2. **Tối ưu hiệu năng Svelte 5**: Sử dụng CSS Custom Properties và GPU transform để đạt 60+ FPS mượt mà.
3. **Giới hạn phong cách Stage**: Theo đúng yêu cầu, chỉ triển khai phong cách **Flow** kinh điển của Folia, loại bỏ các biến thể phức tạp (Steps, Tilt) để đảm bảo trải nghiệm tinh tế, không rối mắt.

## Success Criteria

- [x] Thuật toán `spreadWords` phân rã chính xác tiếng Anh, tiếng Việt, CJK và vượt qua 100% unit tests.
- [x] Dòng lời đang hát trong `LyricsPanel.svelte` chạy chữ phát sáng từng từ theo tiến độ phát nhạc.
- [x] Khoảng nghỉ giữa các câu / intro hiển thị 3 chấm nhịp thở êm ái.
- [x] Now Playing cho phép chuyển đổi sang chế độ `stage` với visualizer Folia Flow chuyển động lò xo nảy tự nhiên.
- [x] `pnpm --dir app check` và `pnpm --dir app test` đạt 100% xanh không có lỗi hoặc cảnh báo.

<!-- slug: running-lyrics-flow -->
