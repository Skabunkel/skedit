use std::fmt;

use starlark_syntax::codemap::Span;
use starlark_syntax::syntax::ast::{
    AstExprP, AstLiteral, AstNoPayload, ArgumentP, CallArgsP, ExprP, StmtP,
};
use starlark_syntax::syntax::module::AstModuleFields;
use starlark_syntax::syntax::AstModule;
use starlark_syntax::syntax::Dialect;

/// Selects which rule and attribute to target in a starlark file.
#[derive(Debug, Clone)]
pub struct RuleSelector {
    /// The rule function name, e.g. "rust_binary"
    pub rule_name: String,
    /// The attribute containing the file list, e.g. "srcs"
    pub attr: String,
    /// Optional: only match rules with this `name = "..."` value
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    Parse(String),
    RuleNotFound {
        rule_name: String,
    },
    AttrNotFound {
        rule_name: String,
        attr: String,
    },
    AttrNotAList {
        rule_name: String,
        attr: String,
    },
    EntryAlreadyPresent {
        entry: String,
    },
    EntryNotFound {
        entry: String,
    },
    /// Multiple rules matched; narrow with `name`
    AmbiguousRule {
        rule_name: String,
        count: usize,
    },
    /// A rule with this name already exists
    RuleAlreadyExists {
        rule_name: String,
        name: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(msg) => write!(f, "parse error: {msg}"),
            Error::RuleNotFound { rule_name } => {
                write!(f, "no rule `{rule_name}` found")
            }
            Error::AttrNotFound {
                rule_name, attr, ..
            } => {
                write!(f, "rule `{rule_name}` has no attribute `{attr}`")
            }
            Error::AttrNotAList {
                rule_name, attr, ..
            } => {
                write!(f, "attribute `{attr}` of rule `{rule_name}` is not a list")
            }
            Error::EntryAlreadyPresent { entry } => {
                write!(f, "`{entry}` is already present")
            }
            Error::EntryNotFound { entry } => write!(f, "`{entry}` not found in list"),
            Error::AmbiguousRule { rule_name, count } => {
                write!(
                    f,
                    "found {count} rules named `{rule_name}`; use `name` to disambiguate"
                )
            }
            Error::RuleAlreadyExists { rule_name, name } => {
                write!(f, "rule `{rule_name}` with name `{name}` already exists")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Information about a matched rule call, with all its arguments extracted.
struct RuleInfo {
    /// The function name, e.g. "rust_binary"
    func_name: String,
    /// Span of the entire call expression
    call_span: Span,
    /// All named arguments in order
    args: Vec<ArgInfo>,
    /// Index of the target attribute in `args`, if it exists
    target_idx: Option<usize>,
}

/// A single named argument in a rule call.
struct ArgInfo {
    name: String,
    value: ArgValue,
}

/// The value of a named argument.
enum ArgValue {
    /// A list of string literals — we can reformat these
    StringList(Vec<String>),
    /// Any other expression — preserve original source text
    Raw(String),
}

/// Parse starlark source and find the target rule call.
fn find_rule(source: &str, selector: &RuleSelector) -> Result<RuleInfo, Error> {
    let module = AstModule::parse(
        "BUILD",
        source.to_owned(),
        &Dialect::Standard,
    )
    .map_err(|e| Error::Parse(e.to_string()))?;

    let stmt = module.statement();
    let mut matches = Vec::new();
    collect_matching_calls(source, &stmt.node, selector, &mut matches);

    if matches.is_empty() {
        return Err(Error::RuleNotFound {
            rule_name: selector.rule_name.clone(),
        });
    }
    if matches.len() > 1 {
        return Err(Error::AmbiguousRule {
            rule_name: selector.rule_name.clone(),
            count: matches.len(),
        });
    }

    Ok(matches.into_iter().next().unwrap())
}

/// Recursively walk statements to find matching rule calls.
fn collect_matching_calls(
    source: &str,
    stmt: &StmtP<AstNoPayload>,
    selector: &RuleSelector,
    results: &mut Vec<RuleInfo>,
) {
    match stmt {
        StmtP::Statements(stmts) => {
            for s in stmts {
                collect_matching_calls(source, &s.node, selector, results);
            }
        }
        StmtP::Expression(expr) => {
            if let Some(result) = try_match_call(source, expr, selector) {
                results.push(result);
            }
        }
        _ => {}
    }
}

/// Check if an expression is a call matching the selector.
fn try_match_call(
    source: &str,
    expr: &AstExprP<AstNoPayload>,
    selector: &RuleSelector,
) -> Option<RuleInfo> {
    let ExprP::Call(func, CallArgsP { args }) = &expr.node else {
        return None;
    };

    let ExprP::Identifier(ident) = &func.node else {
        return None;
    };
    if ident.node.ident != selector.rule_name {
        return None;
    }

    // If selector has a name filter, check the `name` attribute
    if let Some(required_name) = &selector.name {
        let has_matching_name = args.iter().any(|arg| {
            if let ArgumentP::Named(name, value) = &arg.node {
                if name.node == "name" {
                    if let ExprP::Literal(AstLiteral::String(s)) = &value.node {
                        return &s.node == required_name;
                    }
                }
            }
            false
        });
        if !has_matching_name {
            return None;
        }
    }

    let call_span = expr.span;
    let func_name = ident.node.ident.clone();
    let mut extracted_args = Vec::new();
    let mut target_idx = None;

    for arg in args {
        if let ArgumentP::Named(name, value) = &arg.node {
            let arg_name = name.node.clone();
            let is_target = arg_name == selector.attr;

            let arg_value = if let ExprP::List(items) = &value.node {
                // Check if all items are string literals
                let mut strings = Vec::new();
                let mut all_strings = true;
                for item in items {
                    if let ExprP::Literal(AstLiteral::String(s)) = &item.node {
                        strings.push(s.node.clone());
                    } else {
                        all_strings = false;
                        break;
                    }
                }
                if all_strings {
                    ArgValue::StringList(strings)
                } else {
                    ArgValue::Raw(source[span_range(&value.span)].to_string())
                }
            } else {
                ArgValue::Raw(source[span_range(&value.span)].to_string())
            };

            if is_target {
                target_idx = Some(extracted_args.len());
            }
            extracted_args.push(ArgInfo { name: arg_name, value: arg_value });
        }
    }

    Some(RuleInfo {
        func_name,
        call_span,
        args: extracted_args,
        target_idx,
    })
}

/// Format a list of string entries, choosing single-line or multi-line based on count.
fn format_list(entries: &[String], attr_indent: &str) -> String {
    if entries.is_empty() {
        "[]".to_string()
    } else if entries.len() <= 2 {
        let inner: Vec<_> = entries.iter().map(|e| format!("\"{}\"", e)).collect();
        format!("[{}]", inner.join(", "))
    } else {
        let entry_indent = format!("{}    ", attr_indent);
        let mut result = String::from("[\n");
        for entry in entries {
            result.push_str(&entry_indent);
            result.push_str(&format!("\"{}\",\n", entry));
        }
        result.push_str(attr_indent);
        result.push(']');
        result
    }
}

/// Format an argument value.
fn format_arg_value(value: &ArgValue, attr_indent: &str) -> String {
    match value {
        ArgValue::StringList(entries) => format_list(entries, attr_indent),
        ArgValue::Raw(raw) => raw.clone(),
    }
}

/// Rebuild an entire rule call with consistent formatting, then splice it into source.
fn rewrite_rule(source: &str, rule: &RuleInfo) -> String {
    let range = span_range(&rule.call_span);
    let base_indent = detect_indent_at(source, rule.call_span.begin().get() as usize);
    let attr_indent = format!("{}    ", base_indent);

    let mut formatted = format!("{}(\n", rule.func_name);
    for arg in &rule.args {
        formatted.push_str(&attr_indent);
        formatted.push_str(&arg.name);
        formatted.push_str(" = ");
        formatted.push_str(&format_arg_value(&arg.value, &attr_indent));
        formatted.push_str(",\n");
    }
    formatted.push_str(base_indent);
    formatted.push(')');

    let mut result = String::with_capacity(source.len() + formatted.len());
    result.push_str(&source[..range.start]);
    result.push_str(&formatted);
    result.push_str(&source[range.end..]);
    result
}

/// Get the target arg's string list entries, or None if attr doesn't exist.
fn get_target_entries(rule: &RuleInfo) -> Option<&Vec<String>> {
    rule.target_idx.and_then(|idx| {
        match &rule.args[idx].value {
            ArgValue::StringList(entries) => Some(entries),
            ArgValue::Raw(_) => None,
        }
    })
}

/// Add an entry to the specified rule's list attribute.
/// Creates the attribute if it doesn't exist.
/// Reformats the entire rule block with consistent indentation.
pub fn add_entry(source: &str, selector: &RuleSelector, entry: &str) -> Result<String, Error> {
    let mut rule = find_rule(source, selector)?;

    match rule.target_idx {
        Some(idx) => {
            let entries = match &rule.args[idx].value {
                ArgValue::StringList(entries) => entries,
                ArgValue::Raw(_) => {
                    return Err(Error::AttrNotAList {
                        rule_name: selector.rule_name.clone(),
                        attr: selector.attr.clone(),
                    });
                }
            };

            if entries.iter().any(|name| name == entry) {
                return Err(Error::EntryAlreadyPresent {
                    entry: entry.to_string(),
                });
            }

            let mut new_entries = entries.clone();
            new_entries.push(entry.to_string());
            new_entries.sort();
            rule.args[idx].value = ArgValue::StringList(new_entries);
        }
        None => {
            rule.args.push(ArgInfo {
                name: selector.attr.clone(),
                value: ArgValue::StringList(vec![entry.to_string()]),
            });
        }
    }

    Ok(rewrite_rule(source, &rule))
}

/// Remove an entry from the specified rule's list attribute.
/// Removes the attribute entirely if the list becomes empty.
/// Reformats the entire rule block with consistent indentation.
pub fn remove_entry(source: &str, selector: &RuleSelector, entry: &str) -> Result<String, Error> {
    let mut rule = find_rule(source, selector)?;

    let idx = rule.target_idx.ok_or_else(|| Error::AttrNotFound {
        rule_name: selector.rule_name.clone(),
        attr: selector.attr.clone(),
    })?;

    let entries = match &rule.args[idx].value {
        ArgValue::StringList(entries) => entries,
        ArgValue::Raw(_) => {
            return Err(Error::AttrNotAList {
                rule_name: selector.rule_name.clone(),
                attr: selector.attr.clone(),
            });
        }
    };

    let entry_idx = entries
        .iter()
        .position(|name| name == entry)
        .ok_or_else(|| Error::EntryNotFound {
            entry: entry.to_string(),
        })?;

    let mut new_entries = entries.clone();
    new_entries.remove(entry_idx);
    new_entries.sort();

    if new_entries.is_empty() {
        rule.args.remove(idx);
    } else {
        rule.args[idx].value = ArgValue::StringList(new_entries);
    }

    Ok(rewrite_rule(source, &rule))
}

/// List entries in the specified rule's list attribute.
/// Returns an empty list if the attribute doesn't exist.
pub fn list_entries(source: &str, selector: &RuleSelector) -> Result<Vec<String>, Error> {
    let rule = find_rule(source, selector)?;

    match get_target_entries(&rule) {
        Some(entries) => Ok(entries.clone()),
        None if rule.target_idx.is_some() => Err(Error::AttrNotAList {
            rule_name: selector.rule_name.clone(),
            attr: selector.attr.clone(),
        }),
        None => Ok(Vec::new()),
    }
}

/// Create a new rule block and append it to the source.
/// Errors if a rule of the same type and name already exists.
pub fn create_rule(source: &str, rule_name: &str, name: &str) -> Result<String, Error> {
    // Check for duplicates using a selector with the name filter
    let check = RuleSelector {
        rule_name: rule_name.to_string(),
        attr: String::new(),
        name: Some(name.to_string()),
    };
    match find_rule(source, &check) {
        Ok(_) => {
            return Err(Error::RuleAlreadyExists {
                rule_name: rule_name.to_string(),
                name: name.to_string(),
            });
        }
        Err(Error::RuleNotFound { .. }) | Err(Error::AmbiguousRule { .. }) => {
            // RuleNotFound is expected; AmbiguousRule means multiple rules exist
            // but none matched the name, which is fine
        }
        Err(e) => return Err(e),
    }

    let block = format!("{}(\n    name = \"{}\",\n)", rule_name, name);

    let mut result = source.to_string();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    if !result.is_empty() {
        result.push('\n');
    }
    result.push_str(&block);
    result.push('\n');
    Ok(result)
}

/// Remove a rule by its function name and `name` attribute.
pub fn remove_rule(source: &str, rule_name: &str, name: &str) -> Result<String, Error> {
    let selector = RuleSelector {
        rule_name: rule_name.to_string(),
        attr: String::new(),
        name: Some(name.to_string()),
    };
    let rule = find_rule(source, &selector)?;

    let range = span_range(&rule.call_span);

    // Expand to remove surrounding whitespace/newlines
    let mut start = range.start;
    let mut end = range.end;

    // Consume trailing whitespace and newlines
    while end < source.len() && (source.as_bytes()[end] == b'\n' || source.as_bytes()[end] == b' ' || source.as_bytes()[end] == b'\t') {
        end += 1;
    }

    // Consume leading blank lines back to the previous rule/content
    while start > 0 && source.as_bytes()[start - 1] == b'\n' {
        start -= 1;
    }
    // Keep one newline separator if we're not at the start of the file
    if start > 0 && end < source.len() {
        start += 1;
    }

    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..start]);
    result.push_str(&source[end..]);
    Ok(result)
}

/// A rule entry found in the file.
#[derive(Debug, Clone)]
pub struct RuleEntry {
    /// The function name, e.g. "rust_binary"
    pub rule_type: String,
    /// The `name = "..."` value, if present
    pub name: Option<String>,
}

/// List all rule calls in the file.
pub fn list_rules(source: &str) -> Result<Vec<RuleEntry>, Error> {
    let module = AstModule::parse(
        "BUILD",
        source.to_owned(),
        &Dialect::Standard,
    )
    .map_err(|e| Error::Parse(e.to_string()))?;

    let stmt = module.statement();
    let mut entries = Vec::new();
    collect_all_rules(&stmt.node, &mut entries);
    Ok(entries)
}

/// Recursively walk statements to collect all rule calls.
fn collect_all_rules(
    stmt: &StmtP<AstNoPayload>,
    results: &mut Vec<RuleEntry>,
) {
    match stmt {
        StmtP::Statements(stmts) => {
            for s in stmts {
                collect_all_rules(&s.node, results);
            }
        }
        StmtP::Expression(expr) => {
            if let Some(entry) = try_extract_rule(&expr.node) {
                results.push(entry);
            }
        }
        _ => {}
    }
}

/// Try to extract a rule entry from a call expression.
fn try_extract_rule(expr: &ExprP<AstNoPayload>) -> Option<RuleEntry> {
    let ExprP::Call(func, CallArgsP { args }) = expr else {
        return None;
    };
    let ExprP::Identifier(ident) = &func.node else {
        return None;
    };

    let rule_type = ident.node.ident.clone();
    let name = args.iter().find_map(|arg| {
        if let ArgumentP::Named(name, value) = &arg.node {
            if name.node == "name" {
                if let ExprP::Literal(AstLiteral::String(s)) = &value.node {
                    return Some(s.node.clone());
                }
            }
        }
        None
    });

    Some(RuleEntry { rule_type, name })
}

// --- helpers ---

fn span_range(span: &Span) -> std::ops::Range<usize> {
    span.begin().get() as usize..span.end().get() as usize
}

/// Detect the indentation (leading whitespace) of the line containing the given byte position.
fn detect_indent_at(source: &str, pos: usize) -> &str {
    let line_start = source[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line = &source[line_start..];
    let indent_len = line.len() - line.trim_start().len();
    &source[line_start..line_start + indent_len]
}


#[cfg(test)]
mod tests {
    use super::*;

    fn selector(rule: &str, attr: &str) -> RuleSelector {
        RuleSelector {
            rule_name: rule.to_string(),
            attr: attr.to_string(),
            name: None,
        }
    }

    fn named_selector(rule: &str, attr: &str, name: &str) -> RuleSelector {
        RuleSelector {
            rule_name: rule.to_string(),
            attr: attr.to_string(),
            name: Some(name.to_string()),
        }
    }

    #[test]
    fn list_entries_basic() {
        let source = r#"
rust_binary(
    name = "foo",
    srcs = [
        "main.rs",
        "lib.rs",
    ],
)
"#;
        let sel = selector("rust_binary", "srcs");
        let files = list_entries(source, &sel).unwrap();
        assert_eq!(files, vec!["main.rs", "lib.rs"]);
    }

    #[test]
    fn add_entry_multiline() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = [
        "main.rs",
    ],
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = add_entry(source, &sel, "util.rs").unwrap();
        assert!(result.contains("\"util.rs\""));
        // Verify it parses and has the new file
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["main.rs", "util.rs"]);
    }

    #[test]
    fn add_entry_empty_list() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = [],
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = add_entry(source, &sel, "main.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["main.rs"]);
    }

    #[test]
    fn add_entry_already_present() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = ["main.rs"],
)"#;
        let sel = selector("rust_binary", "srcs");
        let err = add_entry(source, &sel, "main.rs").unwrap_err();
        assert!(matches!(err, Error::EntryAlreadyPresent { .. }));
    }

    #[test]
    fn add_multople_already_present() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = [],
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = add_entry(source, &sel, "main0.rs").unwrap();
        let result = add_entry(&result, &sel, "main1.rs").unwrap();
        let result = add_entry(&result, &sel, "main2.rs").unwrap();
        let result = add_entry(&result, &sel, "main3.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["main0.rs", "main1.rs", "main2.rs", "main3.rs"]);
    }

    #[test]
    fn remove_entry_multiline() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = [
        "main.rs",
        "util.rs",
        "lib.rs",
    ],
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = remove_entry(source, &sel, "util.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["lib.rs", "main.rs"]);
    }

    #[test]
    fn remove_entry_not_found() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = ["main.rs"],
)"#;
        let sel = selector("rust_binary", "srcs");
        let err = remove_entry(source, &sel, "nope.rs").unwrap_err();
        assert!(matches!(err, Error::EntryNotFound { .. }));
    }

    #[test]
    fn name_selector_disambiguates() {
        let source = r#"
rust_binary(
    name = "foo",
    srcs = ["foo.rs"],
)

rust_binary(
    name = "bar",
    srcs = ["bar.rs"],
)
"#;
        let sel = named_selector("rust_binary", "srcs", "bar");
        let files = list_entries(source, &sel).unwrap();
        assert_eq!(files, vec!["bar.rs"]);
    }

    #[test]
    fn ambiguous_rule_error() {
        let source = r#"
rust_binary(
    name = "foo",
    srcs = ["foo.rs"],
)

rust_binary(
    name = "bar",
    srcs = ["bar.rs"],
)
"#;
        let sel = selector("rust_binary", "srcs");
        let err = list_entries(source, &sel).unwrap_err();
        assert!(matches!(err, Error::AmbiguousRule { count: 2, .. }));
    }

    #[test]
    fn rule_not_found() {
        let source = r#"
cc_binary(
    name = "foo",
    srcs = ["main.cc"],
)
"#;
        let sel = selector("rust_binary", "srcs");
        let err = list_entries(source, &sel).unwrap_err();
        assert!(matches!(err, Error::RuleNotFound { .. }));
    }

    #[test]
    fn remove_single_line_middle() {
        let source = r#"rust_binary(name = "x", srcs = ["a.rs", "b.rs", "c.rs"])"#;
        let sel = selector("rust_binary", "srcs");
        let result = remove_entry(source, &sel, "b.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["a.rs", "c.rs"]);
    }

    #[test]
    fn remove_single_line_last() {
        let source = r#"rust_binary(name = "x", srcs = ["a.rs", "b.rs"])"#;
        let sel = selector("rust_binary", "srcs");
        let result = remove_entry(source, &sel, "b.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["a.rs"]);
    }

    #[test]
    fn remove_leaves_empty_list() {
        let source = r#"rust_binary(
    name = "x",
    srcs = [
        "only.rs",
    ],
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = remove_entry(source, &sel, "only.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert!(files.is_empty());
        // Attribute should be removed entirely
        assert!(!result.contains("srcs"));
    }

    #[test]
    fn add_creates_missing_attr() {
        let source = r#"rust_binary(
    name = "foo",
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = add_entry(source, &sel, "main.rs").unwrap();
        let files = list_entries(&result, &sel).unwrap();
        assert_eq!(files, vec!["main.rs"]);
    }

    #[test]
    fn remove_deletes_empty_attr() {
        let source = r#"rust_binary(
    name = "foo",
    srcs = ["only.rs"],
)"#;
        let sel = selector("rust_binary", "srcs");
        let result = remove_entry(source, &sel, "only.rs").unwrap();
        assert!(!result.contains("srcs"));
        // Should still parse
        let files = list_entries(&result, &sel).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn list_missing_attr_returns_empty() {
        let source = r#"rust_binary(
    name = "foo",
)"#;
        let sel = selector("rust_binary", "srcs");
        let files = list_entries(source, &sel).unwrap();
        assert!(files.is_empty());
    }
}
