//! LRC text: `[mm:ss.xx]` line stamps (several may share one line), an optional `[offset:ms]`,
//! metadata tags that are skipped, and enhanced-LRC word stamps (`<mm:ss.xx>`) that are
//! dropped from the text. Text with no line stamps at all is plain, unsynced lyrics.

use crate::{Line, Lyrics};

/// Parses LRC or plain text. `None` when nothing but blank lines and metadata remain.
pub fn parse(text: &str, source: &str) -> Option<Lyrics> {
    let text = text.trim_start_matches('\u{feff}');
    let mut offset_ms: i64 = 0;
    let mut timed: Vec<(i64, String)> = Vec::new();
    let mut plain: Vec<String> = Vec::new();

    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps: Vec<i64> = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(end) = body.find(']') else { break };
            let tag = &body[..end];
            if let Some(ms) = timestamp(tag) {
                stamps.push(ms);
            } else if let Some(value) = tag.strip_prefix("offset:") {
                offset_ms = value.trim().parse().unwrap_or(0);
            } else if !is_metadata(tag) {
                break;
            }
            rest = body[end + 1..].trim_start();
        }
        let words = strip_word_stamps(rest);
        if stamps.is_empty() {
            // A metadata-only line ("[ar:Someone]") leaves nothing; anything else is plain text.
            if raw.trim_start().starts_with('[') && words.is_empty() {
                continue;
            }
            plain.push(words);
        } else {
            for ms in stamps {
                timed.push((ms, words.clone()));
            }
        }
    }

    if !timed.is_empty() {
        // LRC's offset is how much earlier the lyrics should show: positive moves them up.
        timed.sort_by_key(|(ms, _)| *ms);
        let lines = timed
            .into_iter()
            .map(|(ms, text)| Line {
                // Stamps and offset come from the file or a server: clamp, never overflow.
                start_ms: ms.saturating_sub(offset_ms).clamp(0, i64::from(u32::MAX)) as u32,
                text,
            })
            .collect();
        return Some(Lyrics::synced(source, lines));
    }
    Lyrics::plain(source, &plain.join("\n"))
}

/// `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx` or `mm:ss:xx` in milliseconds.
fn timestamp(tag: &str) -> Option<i64> {
    let (min, rest) = tag.split_once(':')?;
    let min: i64 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.find(['.', ':']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    if sec.is_empty() || !sec.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let sec: i64 = sec.parse().ok()?;
    let frac_ms = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().ok()? * 100,
        2 => frac.parse::<i64>().ok()? * 10,
        _ => frac[..3].parse::<i64>().ok()?,
    };
    if min < 0 || sec >= 60 {
        return None;
    }
    let ms = min.checked_mul(60_000)?.checked_add(sec * 1000 + frac_ms)?;
    (ms <= i64::from(u32::MAX)).then_some(ms)
}

/// `[ar:…]`, `[ti:…]`, `[by:…]`, `[length:…]` and the like.
fn is_metadata(tag: &str) -> bool {
    tag.split_once(':').is_some_and(|(key, _)| {
        !key.is_empty() && key.len() <= 10 && key.bytes().all(|b| b.is_ascii_alphabetic())
    })
}

/// Removes `<mm:ss.xx>` word stamps, keeping the words.
fn strip_word_stamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('>') {
            Some(j) if timestamp(&after[..j]).is_some() => rest = &after[j + 1..],
            _ => {
                out.push('<');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synced_lines_are_sorted_with_repeated_stamps_and_offset() {
        let text = "\u{feff}[ti:Song]\n[ar:Someone]\n[offset:500]\n[00:12.30][01:02.00]Chorus\n[00:05.1]Verse one\n[00:20]\n";
        let l = parse(text, "test").unwrap();
        assert!(l.synced);
        let got: Vec<(u32, &str)> = l
            .lines
            .iter()
            .map(|x| (x.start_ms, x.text.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (4600, "Verse one"),
                (11800, "Chorus"),
                (19500, ""),
                (61500, "Chorus")
            ]
        );
    }

    #[test]
    fn word_stamps_are_dropped_and_three_digit_fractions_kept() {
        let l = parse("[00:01.234]<00:01.234>Hello <00:01.800>world", "t").unwrap();
        assert_eq!(l.lines[0].start_ms, 1234);
        assert_eq!(l.lines[0].text, "Hello world");
    }

    #[test]
    fn text_without_stamps_is_plain() {
        let l = parse("First line\n\nSecond <b>line</b>\n", "t").unwrap();
        assert!(!l.synced);
        let texts: Vec<&str> = l.lines.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["First line", "", "Second <b>line</b>"]);
    }

    #[test]
    fn metadata_only_is_nothing() {
        assert!(parse("[ar:Someone]\n[ti:Song]\n\n", "t").is_none());
        assert!(parse("", "t").is_none());
    }

    #[test]
    fn absurd_stamps_and_offsets_do_not_overflow() {
        let l = parse(
            "[offset:-9223372036854775808]
[00:01.00]A
[99999999999999:00]B",
            "t",
        )
        .unwrap();
        let got: Vec<(u32, &str)> = l
            .lines
            .iter()
            .map(|x| (x.start_ms, x.text.as_str()))
            .collect();
        // The impossible stamp is not a stamp; the offset pushes the real one to the end.
        assert_eq!(got, vec![(u32::MAX, "A")]);
    }

    #[test]
    fn bracketed_text_that_is_not_a_stamp_stays() {
        let l = parse("[Chorus]\nLa la la", "t").unwrap();
        assert!(!l.synced);
        assert_eq!(l.lines[0].text, "[Chorus]");
    }
}
