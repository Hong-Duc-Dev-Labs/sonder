//! TTML lyrics (the Apple Music format some services return): one `<p begin=…>` per line,
//! its words in `<span>`s. Only line starts are kept. Read by hand: the documents are small
//! and regular, and only `p`'s `begin` and its text matter.

use crate::{Line, Lyrics};

pub fn parse(doc: &str, source: &str) -> Option<Lyrics> {
    let body = &doc[doc.find("<body").unwrap_or(0)..];
    let mut lines = Vec::new();
    let mut rest = body;
    while let Some(i) = find_open(rest, "p") {
        let after = &rest[i..];
        let Some(head_end) = after.find('>') else {
            break;
        };
        let head = &after[..head_end];
        let Some(close) = after.find("</p>") else {
            break;
        };
        let inner = &after[head_end + 1..close];
        if let Some(start) = attr(head, "begin").and_then(clock) {
            lines.push(Line {
                start_ms: start,
                text: text_of(inner),
            });
        }
        rest = &after[close + 4..];
    }
    if lines.is_empty() {
        return None;
    }
    lines.sort_by_key(|l| l.start_ms);
    Some(Lyrics::synced(source, lines))
}

/// Index of the next `<tag` followed by whitespace or `>`.
fn find_open(s: &str, tag: &str) -> Option<usize> {
    let pat = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = s[from..].find(&pat) {
        let at = from + i;
        match s.as_bytes().get(at + pat.len()) {
            Some(b' ' | b'>' | b'\t' | b'\n' | b'\r') => return Some(at),
            _ => from = at + pat.len(),
        }
    }
    None
}

fn attr<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!(" {name}=\"");
    let i = head.find(&pat)? + pat.len();
    let end = head[i..].find('"')?;
    Some(&head[i..i + end])
}

/// TTML clock values: `12.345`, `1:02.345`, `1:02:03.4` or `12.3s`.
fn clock(v: &str) -> Option<u32> {
    let v = v.trim().trim_end_matches('s');
    let mut secs = 0.0f64;
    for part in v.split(':') {
        secs = secs * 60.0 + part.parse::<f64>().ok()?;
    }
    (secs.is_finite() && secs >= 0.0).then(|| (secs * 1000.0).round() as u32)
}

/// The line's text: tags removed, entities decoded, whitespace collapsed.
fn text_of(inner: &str) -> String {
    let mut raw = String::with_capacity(inner.len());
    let mut in_tag = false;
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => raw.push(c),
            _ => {}
        }
    }
    let decoded = decode_entities(&raw);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(end) = after.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let name = &after[..end];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_line_starts_and_word_text() {
        let doc = r#"<tt xmlns="http://www.w3.org/ns/ttml"><head><p begin="0">ignored</p></head><body dur="3:53.7"><div begin="9.7"><p begin="9.731" end="12.1"><span begin="9.731">The</span> <span begin="9.9">club</span> <span>isn&apos;t</span></p><p begin="1:02.5" end="1:04"><span>Rock &amp; roll</span></p></div></body></tt>"#;
        let l = parse(doc, "t").unwrap();
        assert!(l.synced);
        let got: Vec<(u32, &str)> = l
            .lines
            .iter()
            .map(|x| (x.start_ms, x.text.as_str()))
            .collect();
        assert_eq!(got, vec![(9731, "The club isn't"), (62500, "Rock & roll")]);
    }

    #[test]
    fn clock_forms() {
        assert_eq!(clock("12.345"), Some(12345));
        assert_eq!(clock("1:02:03.4"), Some(3_723_400));
        assert_eq!(clock("4.5s"), Some(4500));
        assert_eq!(clock("x"), None);
    }

    #[test]
    fn no_lines_is_nothing() {
        assert!(parse("<tt><body></body></tt>", "t").is_none());
    }
}
