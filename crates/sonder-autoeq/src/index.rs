//! AutoEQ's `results/INDEX.md`: one Markdown list item per result, and the search over it.
//!
//! A line reads `- [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by
//! oratory1990`, with ` on <rig>` after the source when it measured on several rigs. The link
//! is percent-encoded but may hold parentheses, so the path ends at the last `) by `.

/// One AutoEQ result: a headphone as one source measured it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Headphone name, as AutoEQ writes it (variants in parentheses).
    pub name: String,
    /// Who measured it (oratory1990, crinacle, Rtings…).
    pub source: String,
    /// The measurement rig, when the source used several.
    pub rig: Option<String>,
    /// Percent-encoded folder under `results/`, without the leading `./`.
    pub path: String,
}

impl Entry {
    /// URL of the result's `<name> ParametricEQ.txt` under `results`, or `None` for a path
    /// that could step outside it.
    pub fn parametric_eq_url(&self, results: &str) -> Option<String> {
        let safe = !self.path.is_empty()
            && !self.path.contains(['?', '#', '\\'])
            && self
                .path
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        if !safe {
            return None;
        }
        let folder = self.path.rsplit('/').next()?;
        Some(format!(
            "{results}/{}/{folder}%20ParametricEQ.txt",
            self.path
        ))
    }
}

/// Every result listed in `text`; lines that are not results are skipped.
pub fn parse_index(text: &str) -> Vec<Entry> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Entry> {
    let rest = line.trim().strip_prefix("- [")?;
    let (name, rest) = rest.split_once("](./")?;
    let (path, credit) = rest.rsplit_once(") by ")?;
    let (source, rig) = match credit.split_once(" on ") {
        Some((source, rig)) => (source, Some(rig.trim().to_owned())),
        None => (credit, None),
    };
    let name = name.trim();
    if name.is_empty() || path.is_empty() {
        return None;
    }
    Some(Entry {
        name: name.to_owned(),
        source: source.trim().to_owned(),
        rig,
        path: path.to_owned(),
    })
}

/// Lowercase letters and digits only: "HD 600", "HD-600" and "hd600" read the same.
fn compact(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// AutoEQ's own recommendation order for sources; the rest follow alphabetically.
fn source_rank(source: &str) -> usize {
    match source {
        "oratory1990" => 0,
        "crinacle" => 1,
        _ => 2,
    }
}

/// Up to `limit` entries whose name (or source) holds every word of `query`, best first: an
/// exact name, then names starting or ending with the query (the model itself, not a variant),
/// then names with a word starting with its first word; within each, oratory1990 and crinacle
/// first, then shorter names.
pub fn search(entries: &[Entry], query: &str, limit: usize) -> Vec<Entry> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(compact)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let whole: String = words.concat();
    let mut found: Vec<(usize, &Entry)> = entries
        .iter()
        .filter_map(|e| {
            let name = compact(&e.name);
            let haystack = format!("{name} {}", compact(&e.source));
            let hit =
                haystack.contains(&whole) || words.iter().all(|w| haystack.contains(w.as_str()));
            if !hit {
                return None;
            }
            let rank = if name == whole {
                0
            } else if name.starts_with(&whole) || name.ends_with(&whole) {
                1
            } else if e
                .name
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| compact(word).starts_with(&words[0]))
            {
                2
            } else {
                3
            };
            Some((rank, e))
        })
        .collect();
    found.sort_by(|(ra, a), (rb, b)| {
        ra.cmp(rb)
            .then(source_rank(&a.source).cmp(&source_rank(&b.source)))
            .then(a.name.len().cmp(&b.name.len()))
            .then(a.name.cmp(&b.name))
            .then(a.source.cmp(&b.source))
    });
    found
        .into_iter()
        .take(limit)
        .map(|(_, e)| e.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "# Index\nThis is a list of all equalization profiles. Target is in parentheses if there are results with multiple targets\nfrom the same source.\n\n\
        - [Sennheiser HD 600](./Rtings/Bruel%20&%20Kjaer%205128%20over-ear/Sennheiser%20HD%20600) by Rtings on Bruel & Kjaer 5128\n\
        - [Sennheiser HD 600](./oratory1990/over-ear/Sennheiser%20HD%20600) by oratory1990\n\
        - [Sennheiser HD 600 (2020)](./crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600%20(2020)) by crinacle on GRAS 43AG-7\n\
        - [Sennheiser HD 650](./oratory1990/over-ear/Sennheiser%20HD%20650) by oratory1990\n\
        - [Koss KSC75 (HD 600 pads)](./Filk/over-ear/Koss%20KSC75%20(HD%20600%20pads)) by Filk\n\
        - [1MORE Aero (ANC Off)](./HypetheSonics/GRAS%20RA0045%20in-ear/1MORE%20Aero%20(ANC%20Off)) by HypetheSonics on GRAS RA0045\n";

    #[test]
    fn lines_parse_into_name_source_rig_and_path() {
        let entries = parse_index(INDEX);
        assert_eq!(entries.len(), 6);
        assert_eq!(
            entries[2],
            Entry {
                name: "Sennheiser HD 600 (2020)".into(),
                source: "crinacle".into(),
                rig: Some("GRAS 43AG-7".into()),
                path: "crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600%20(2020)".into(),
            }
        );
        assert_eq!(entries[1].rig, None);
        assert_eq!(entries[0].rig.as_deref(), Some("Bruel & Kjaer 5128"));
    }

    #[test]
    fn urls_point_at_the_parametric_file() {
        let e = &parse_index(INDEX)[2];
        assert_eq!(
            e.parametric_eq_url("https://x/results").unwrap(),
            "https://x/results/crinacle/GRAS%2043AG-7%20over-ear/Sennheiser%20HD%20600%20(2020)/Sennheiser%20HD%20600%20(2020)%20ParametricEQ.txt"
        );
        for path in [
            "",
            "../secrets",
            "a//b",
            "a/./b",
            "a/b?x=1",
            "a/b#c",
            r"a\b",
        ] {
            let bad = Entry {
                path: path.into(),
                ..e.clone()
            };
            assert_eq!(bad.parametric_eq_url("https://x/results"), None, "{path}");
        }
    }

    #[test]
    fn search_ranks_exact_names_and_preferred_sources_first() {
        let entries = parse_index(INDEX);
        let found = search(&entries, "hd 600", 10);
        let order: Vec<(&str, &str)> = found
            .iter()
            .map(|e| (e.name.as_str(), e.source.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("Sennheiser HD 600", "oratory1990"),
                ("Sennheiser HD 600", "Rtings"),
                ("Sennheiser HD 600 (2020)", "crinacle"),
                ("Koss KSC75 (HD 600 pads)", "Filk"),
            ]
        );
        assert_eq!(search(&entries, "sennheiser hd600", 10).len(), 3);
        assert_eq!(
            search(&entries, "Sennheiser HD 600", 1)[0].source,
            "oratory1990"
        );
    }

    #[test]
    fn search_ignores_case_punctuation_and_matches_sources() {
        let entries = parse_index(INDEX);
        assert_eq!(search(&entries, "1more aero", 10).len(), 1);
        assert_eq!(search(&entries, "HD-650", 10)[0].name, "Sennheiser HD 650");
        assert_eq!(search(&entries, "650 oratory", 10).len(), 1);
        assert!(search(&entries, "   ", 10).is_empty());
        assert!(search(&entries, "nothing like this", 10).is_empty());
        assert_eq!(search(&entries, "sennheiser", 2).len(), 2);
    }
}
