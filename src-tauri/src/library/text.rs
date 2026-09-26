//! Text folding for grouping, sorting and searching library names.
//!
//! "Sigur Rós", "SIGUR ROS" and "sigur ros" must land in the same artist, sort
//! together and all match a search for "ros". Folding lowercases, removes
//! accents and collapses whitespace; sort keys additionally move leading
//! English articles out of the way ("The Quiet Engines" sorts under Q).

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Case- and accent-insensitive form of `s`, with whitespace collapsed.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        // Letters that do not decompose into a base letter plus a mark.
        match c {
            'ø' | 'Ø' => out.push('o'),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ß' => out.push_str("ss"),
            'đ' | 'Đ' | 'ð' | 'Ð' => out.push('d'),
            'ł' | 'Ł' => out.push('l'),
            'þ' | 'Þ' => out.push_str("th"),
            'ı' => out.push('i'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

const ARTICLES: &[&str] = &["the ", "a ", "an "];

/// Key for alphabetical sorting: folded, without a leading article.
pub fn sort_key(s: &str) -> String {
    let folded = fold(s);
    for article in ARTICLES {
        if let Some(rest) = folded.strip_prefix(article)
            && !rest.is_empty()
        {
            return rest.to_owned();
        }
    }
    folded
}

/// Turns free text into an FTS5 query matching every word as a prefix, e.g.
/// `rain stud` → `"rain"* "stud"*`. Returns `None` if nothing searchable is
/// left. Quotes are doubled so user input can never alter the query syntax.
pub fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '/' | '.' | ',' | '(' | ')' | '[' | ']'))
        .map(fold)
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Escapes `%`, `_` and `\` for a `LIKE … ESCAPE '\'` pattern.
pub fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_accents_and_whitespace() {
        assert_eq!(fold("  Sigur   Rós "), "sigur ros");
        assert_eq!(fold("SOLVEIG AUNE"), "solveig aune");
        assert_eq!(fold("Gunnhild Sæter"), "gunnhild saeter");
        assert_eq!(fold("Mötley Crüe"), "motley crue");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("Øystein Łukasz"), "oystein lukasz");
        assert_eq!(fold("ﬁne"), "fine", "compatibility forms are expanded");
        assert_eq!(fold("坂本龍一"), "坂本龍一");
    }

    #[test]
    fn sort_keys_ignore_leading_articles() {
        assert_eq!(sort_key("The Quiet Engines"), "quiet engines");
        assert_eq!(sort_key("A Winged Victory"), "winged victory");
        assert_eq!(sort_key("The"), "the", "a bare article is kept");
        assert_eq!(sort_key("Theatre"), "theatre");
    }

    #[test]
    fn builds_safe_prefix_queries() {
        assert_eq!(fts_query("Rain  Stúd").as_deref(), Some("\"rain\"* \"stud\"*"));
        assert_eq!(fts_query("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        assert_eq!(fts_query("  - "), None);
    }

    #[test]
    fn escapes_like_wildcards() {
        assert_eq!(like_escape("100%_a\\b"), "100\\%\\_a\\\\b");
    }
}
