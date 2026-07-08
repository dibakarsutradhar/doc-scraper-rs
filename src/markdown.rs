#[derive(Debug, Clone)]
pub struct Section {
    pub title: String,
    pub pages: Vec<(String, String, String)>, // (title, relative url, file path relative to dir)
}

/// Groups pages by their top-level URL path segment ("markets/ethena-usde" → "markets")
/// and renders an index.md with one H2 section per group.
pub fn generate_index(sections: &[Section]) -> String {
    let mut out = String::new();
    out.push_str("# Documentation Index\n\n");
    for section in sections {
        out.push_str(&format!("## {}\n\n", section.title));
        for (title, url, _relpath) in &section.pages {
            out.push_str(&format!("- [{}]({})\n", title, url));
        }
        out.push('\n');
    }
    out
}

/// Lines: `- [Title](url)` (no description if None).
pub fn generate_llms_txt(pages: &[(String, String, Option<String>)]) -> String {
    let mut out = String::new();
    for (title, url, descr) in pages {
        match descr {
            Some(d) if !d.trim().is_empty() => {
                out.push_str(&format!("- [{}]({}): {}\n", title, url, d.trim()));
            }
            _ => out.push_str(&format!("- [{}]({})\n", title, url)),
        }
    }
    out
}

/// Helper that turns a flat page list into Section records grouped by URL top-level.
/// `pages` is (title, full relative url, rel path on disk).
pub fn group_into_sections(pages: Vec<(String, String, String)>) -> Vec<Section> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for (title, url, relpath) in pages {
        let top = url
            .trim_start_matches('/')
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();
        let key = if top.is_empty() { "index".into() } else { top };
        map.entry(key).or_default().push((title, url, relpath));
    }
    map.into_iter()
        .map(|(title, pages)| Section { title, pages })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(title: &str, url: &str) -> (String, String, String) {
        (title.into(), url.into(), format!("{}.md", url))
    }

    #[test]
    fn group_into_sections_orders_alphabetically() {
        let pages = vec![
            p("Protocol Overview", "technical-documentation/protocol-overview"),
            p("Why Strata", "introduction/why-strata"),
            p("Ethena USDe", "markets/ethena-usde"),
            p("Senior Tranche", "introduction/senior-tranche"),
        ];
        let sections = group_into_sections(pages);
        assert_eq!(sections[0].title, "introduction");
        assert_eq!(sections[0].pages.len(), 2);
        assert_eq!(sections[1].title, "markets");
        assert_eq!(sections[2].title, "technical-documentation");
    }

    #[test]
    fn generate_index_emits_h2_per_section() {
        let sections = group_into_sections(vec![
            p("Foo", "intro/foo"),
            p("Bar", "markets/bar"),
        ]);
        let md = generate_index(&sections);
        assert!(md.starts_with("# Documentation Index\n\n"));
        assert!(md.contains("## intro\n"));
        assert!(md.contains("## markets\n"));
        assert!(md.contains("- [Foo](intro/foo)\n"));
    }

    #[test]
    fn generate_llms_txt_with_and_without_description() {
        let pages = vec![
            ("A".into(), "/a".into(), Some("First page".into())),
            ("B".into(), "/b".into(), None),
            ("C".into(), "/c".into(), Some("".into())),
        ];
        let s = generate_llms_txt(&pages);
        assert!(s.contains("- [A](/a): First page\n"));
        assert!(s.contains("- [B](/b)\n"));
        assert!(s.contains("- [C](/c)\n"));
    }
}
