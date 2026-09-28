use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayValue {
    True,
    False,
    All,
}

impl DisplayValue {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ListQuery {
    pub query: Option<String>,
    pub fields: Option<String>,
    pub page_size: Option<u32>,
    pub offset: Option<u32>,
    pub display_value: Option<DisplayValue>,
    pub exclude_reference_link: Option<bool>,
    pub suppress_pagination_header: Option<bool>,
    pub view: Option<String>,
    pub query_category: Option<String>,
    pub query_no_domain: Option<bool>,
    pub no_count: Option<bool>,
}

#[derive(Debug, Default, Clone)]
pub struct GetQuery {
    pub fields: Option<String>,
    pub display_value: Option<DisplayValue>,
    pub exclude_reference_link: Option<bool>,
    pub view: Option<String>,
    pub query_no_domain: Option<bool>,
}

#[derive(Debug, Default, Clone)]
pub struct WriteQuery {
    pub fields: Option<String>,
    pub display_value: Option<DisplayValue>,
    pub exclude_reference_link: Option<bool>,
    pub input_display_value: Option<bool>,
    pub suppress_auto_sys_field: Option<bool>,
    pub view: Option<String>,
    pub query_no_domain: Option<bool>, // PATCH/PUT only; POST ignores
}

#[derive(Debug, Default, Clone)]
pub struct DeleteQuery {
    pub query_no_domain: Option<bool>,
}

fn push(pairs: &mut Vec<(String, String)>, key: &str, val: Option<String>) {
    if let Some(v) = val {
        pairs.push((key.into(), v));
    }
}

fn push_bool(pairs: &mut Vec<(String, String)>, key: &str, val: Option<bool>) {
    if let Some(v) = val {
        pairs.push((key.into(), v.to_string()));
    }
}

fn push_u32(pairs: &mut Vec<(String, String)>, key: &str, val: Option<u32>) {
    if let Some(v) = val {
        pairs.push((key.into(), v.to_string()));
    }
}

impl ListQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push(&mut p, "sysparm_query", self.query.clone());
        push(&mut p, "sysparm_fields", self.fields.clone());
        push_u32(&mut p, "sysparm_limit", self.page_size);
        push_u32(&mut p, "sysparm_offset", self.offset);
        push(
            &mut p,
            "sysparm_display_value",
            self.display_value.map(|d| d.as_str().to_string()),
        );
        push_bool(
            &mut p,
            "sysparm_exclude_reference_link",
            self.exclude_reference_link,
        );
        push_bool(
            &mut p,
            "sysparm_suppress_pagination_header",
            self.suppress_pagination_header,
        );
        push(&mut p, "sysparm_view", self.view.clone());
        push(
            &mut p,
            "sysparm_query_category",
            self.query_category.clone(),
        );
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        push_bool(&mut p, "sysparm_no_count", self.no_count);
        p
    }
}

impl GetQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push(&mut p, "sysparm_fields", self.fields.clone());
        push(
            &mut p,
            "sysparm_display_value",
            self.display_value.map(|d| d.as_str().to_string()),
        );
        push_bool(
            &mut p,
            "sysparm_exclude_reference_link",
            self.exclude_reference_link,
        );
        push(&mut p, "sysparm_view", self.view.clone());
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        p
    }
}

impl WriteQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push(&mut p, "sysparm_fields", self.fields.clone());
        push(
            &mut p,
            "sysparm_display_value",
            self.display_value.map(|d| d.as_str().to_string()),
        );
        push_bool(
            &mut p,
            "sysparm_exclude_reference_link",
            self.exclude_reference_link,
        );
        push_bool(
            &mut p,
            "sysparm_input_display_value",
            self.input_display_value,
        );
        push_bool(
            &mut p,
            "sysparm_suppress_auto_sys_field",
            self.suppress_auto_sys_field,
        );
        push(&mut p, "sysparm_view", self.view.clone());
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        p
    }
}

impl DeleteQuery {
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let mut p = Vec::new();
        push_bool(&mut p, "sysparm_query_no_domain", self.query_no_domain);
        p
    }
}

/// Split an encoded query into its `^`-separated terms, as raw slices.
///
/// `^^` is ServiceNow's escape for a literal caret inside a value, so it never
/// separates. Empty terms (a trailing `^`, `^^^`-style doubling at a boundary)
/// are dropped: rejoining the result with `^` yields an equivalent query with no
/// dangling separator, which matters because appending `^term` to a query that
/// already ends in `^` would produce `^^term` — an escaped caret, not a new term.
fn encoded_terms(q: &str) -> Vec<&str> {
    let bytes = q.as_bytes();
    let mut terms = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'^' {
            if bytes.get(i + 1) == Some(&b'^') {
                i += 2;
                continue;
            }
            terms.push(&q[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    terms.push(&q[start..]);
    terms.retain(|t| !t.is_empty());
    terms
}

/// True when the encoded query carries its own sort (`ORDERBY…`/`ORDERBYDESC…`).
///
/// Keyset pagination needs `ORDERBYsys_id` to be the *primary* sort, so a
/// caller's sort means `--all` must page by offset instead. Matched per term,
/// not as a substring, so `short_descriptionLIKEORDERBY` is not a sort.
pub fn has_orderby(q: &str) -> bool {
    encoded_terms(q).iter().any(|t| t.starts_with("ORDERBY"))
}

/// The `sysparm_query` for one keyset page: the caller's filter, restricted to
/// rows after `cursor`, sorted by `sys_id`.
///
/// **The cursor term is distributed into every `^NQ` segment.** Encoded queries
/// have no parentheses, and the two OR operators bind differently (measured on a
/// Zurich PDI, see CLAUDE.md "Pagination"): a trailing AND term applies to a
/// whole `^OR` group — `a^ORb^sys_id>X` is `(a OR b) AND sys_id>X` — but only to
/// the *last* `^NQ` segment — `a^NQb^sys_id>X` is `a OR (b AND sys_id>X)`. A
/// cursor appended once would never constrain the earlier segments, so every
/// page would re-return them from the top: an endless duplicate loop, not merely
/// a wrong filter. `a^sys_id>X^NQb^sys_id>X` is the correct form. One trailing
/// `ORDERBYsys_id` sorts the whole union (also measured).
pub fn keyset_query(base: Option<&str>, cursor: Option<&str>) -> String {
    let mut segments: Vec<Vec<&str>> = vec![Vec::new()];
    for term in encoded_terms(base.unwrap_or("")) {
        if let Some(rest) = term.strip_prefix("NQ") {
            segments.push(Vec::new());
            if !rest.is_empty() {
                segments.last_mut().expect("just pushed").push(rest);
            }
        } else {
            segments.last_mut().expect("never empty").push(term);
        }
    }
    // An empty segment (a leading `^NQ`) filters nothing on its own; keeping it
    // would add a bare `sys_id>X` disjunct that matches rows outside the filter.
    segments.retain(|s| !s.is_empty());
    if segments.is_empty() {
        segments.push(Vec::new());
    }
    let cursor_term = cursor.map(|c| format!("sys_id>{c}"));
    for seg in &mut segments {
        if let Some(ct) = &cursor_term {
            seg.push(ct);
        }
    }
    segments
        .last_mut()
        .expect("never empty")
        .push("ORDERBYsys_id");
    segments
        .iter()
        .map(|s| s.join("^"))
        .collect::<Vec<_>>()
        .join("^NQ")
}

/// Whether `s` can be spliced into an encoded query as a `sys_id>` cursor
/// without becoming query syntax. sys_ids are 32 hex characters, but a handful
/// of platform rows use readable ids (`global`); both fit.
pub fn is_cursor_safe(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyset_query_without_filter_or_cursor_is_just_the_sort() {
        assert_eq!(keyset_query(None, None), "ORDERBYsys_id");
        assert_eq!(keyset_query(Some(""), None), "ORDERBYsys_id");
        assert_eq!(keyset_query(None, Some("abc")), "sys_id>abc^ORDERBYsys_id");
    }

    #[test]
    fn keyset_query_appends_cursor_and_sort_to_a_plain_filter() {
        assert_eq!(
            keyset_query(Some("active=true^priority=1"), Some("abc")),
            "active=true^priority=1^sys_id>abc^ORDERBYsys_id"
        );
        assert_eq!(
            keyset_query(Some("active=true"), None),
            "active=true^ORDERBYsys_id"
        );
    }

    /// `^OR` groups bind tighter than the trailing AND, so a single appended
    /// cursor constrains the whole group — no rewrite needed.
    #[test]
    fn keyset_query_leaves_or_groups_alone() {
        assert_eq!(
            keyset_query(Some("active=true^ORactive=false"), Some("abc")),
            "active=true^ORactive=false^sys_id>abc^ORDERBYsys_id"
        );
    }

    /// The regression the issue's original plan would have shipped: a cursor
    /// appended once to an `^NQ` query only constrains the last segment.
    #[test]
    fn keyset_query_distributes_cursor_into_every_nq_segment() {
        assert_eq!(
            keyset_query(Some("active=true^NQactive=false"), Some("abc")),
            "active=true^sys_id>abc^NQactive=false^sys_id>abc^ORDERBYsys_id"
        );
        assert_eq!(
            keyset_query(Some("a=1^b=2^NQc=3^ORd=4^NQe=5"), Some("x")),
            "a=1^b=2^sys_id>x^NQc=3^ORd=4^sys_id>x^NQe=5^sys_id>x^ORDERBYsys_id"
        );
        // First page: no cursor, sort still trails the whole union.
        assert_eq!(
            keyset_query(Some("active=true^NQactive=false"), None),
            "active=true^NQactive=false^ORDERBYsys_id"
        );
    }

    /// An empty segment filters nothing; given its own cursor term it would
    /// become a bare `sys_id>X` disjunct matching rows outside the filter.
    #[test]
    fn keyset_query_drops_empty_nq_segments() {
        assert_eq!(
            keyset_query(Some("NQactive=true^NQ"), Some("abc")),
            "active=true^sys_id>abc^ORDERBYsys_id"
        );
    }

    #[test]
    fn keyset_query_drops_a_trailing_separator_instead_of_escaping_the_cursor() {
        assert_eq!(
            keyset_query(Some("active=true^"), Some("abc")),
            "active=true^sys_id>abc^ORDERBYsys_id"
        );
    }

    #[test]
    fn keyset_query_keeps_escaped_carets_inside_values() {
        assert_eq!(
            keyset_query(Some("short_description=a^^NQb"), Some("abc")),
            "short_description=a^^NQb^sys_id>abc^ORDERBYsys_id"
        );
    }

    #[test]
    fn has_orderby_matches_terms_not_substrings() {
        assert!(has_orderby("ORDERBYnumber"));
        assert!(has_orderby("active=true^ORDERBYDESCsys_created_on"));
        assert!(has_orderby("a=1^NQb=2^ORDERBYnumber"));
        assert!(!has_orderby("short_descriptionLIKEORDERBY"));
        assert!(!has_orderby("active=true^ORactive=false"));
        assert!(!has_orderby(""));
    }

    #[test]
    fn cursor_safety_rejects_query_syntax() {
        assert!(is_cursor_safe("46d44a5dc0a8010e0144b1c0d5ffe4d2"));
        assert!(is_cursor_safe("global"));
        assert!(!is_cursor_safe(""));
        assert!(!is_cursor_safe("abc^NQactive=true"));
        assert!(!is_cursor_safe("a b"));
        assert!(!is_cursor_safe(&"a".repeat(33)));
    }

    #[test]
    fn list_query_emits_only_set_pairs() {
        let q = ListQuery {
            query: Some("active=true".into()),
            page_size: Some(10),
            ..Default::default()
        };
        let pairs = q.to_pairs();
        assert_eq!(
            pairs,
            vec![
                ("sysparm_query".into(), "active=true".into()),
                ("sysparm_limit".into(), "10".into()),
            ]
        );
    }

    #[test]
    fn display_value_serialises_as_lowercase_string() {
        let q = ListQuery {
            display_value: Some(DisplayValue::All),
            ..Default::default()
        };
        assert_eq!(
            q.to_pairs(),
            vec![("sysparm_display_value".into(), "all".into())]
        );
    }

    #[test]
    fn write_query_respects_all_fields() {
        let q = WriteQuery {
            fields: Some("a,b".into()),
            input_display_value: Some(true),
            suppress_auto_sys_field: Some(true),
            display_value: Some(DisplayValue::False),
            ..Default::default()
        };
        let pairs = q.to_pairs();
        assert!(pairs.contains(&("sysparm_fields".into(), "a,b".into())));
        assert!(pairs.contains(&("sysparm_input_display_value".into(), "true".into())));
        assert!(pairs.contains(&("sysparm_suppress_auto_sys_field".into(), "true".into())));
        assert!(pairs.contains(&("sysparm_display_value".into(), "false".into())));
    }

    #[test]
    fn empty_query_emits_no_pairs() {
        let q = ListQuery::default();
        assert!(q.to_pairs().is_empty());
    }
}
