//! YAML frontmatter handling for Markdown files.
//!
//! A Zynky document is a Markdown file that may start with a YAML block:
//!
//! ```text
//! ---
//! title: GitHub
//! tags: [dev, git]
//! secure_blob: ZYK1...base64...
//! ---
//! Human readable notes go here.
//! ```
//!
//! The frontmatter is exposed to the frontend as a JSON object so the UI never
//! needs to understand YAML.

use serde_json::{Map, Value};
use thiserror::Error;

const DELIMITER: &str = "---";

#[derive(Debug, Error)]
pub enum FrontmatterError {
    #[error("frontmatter is not valid YAML: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error("frontmatter must be a YAML mapping")]
    NotAMapping,
    #[error("frontmatter opened with '---' but was never closed")]
    Unterminated,
}

/// A parsed Markdown document: structured metadata plus the free-form body.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub frontmatter: Map<String, Value>,
    pub body: String,
}

/// Split a Markdown string into frontmatter + body.
///
/// A file without a leading `---` line is treated as having no frontmatter.
/// Line endings are normalised to `\n` in the returned body.
pub fn parse(raw: &str) -> Result<Document, FrontmatterError> {
    let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let normalised = text.replace("\r\n", "\n");
    let mut lines = normalised.split_inclusive('\n');

    let first = match lines.next() {
        Some(line) => line,
        None => return Ok(Document::default()),
    };
    if first.trim_end() != DELIMITER {
        return Ok(Document {
            frontmatter: Map::new(),
            body: normalised,
        });
    }

    let mut yaml = String::new();
    let mut closed = false;
    let mut consumed = first.len();
    for line in lines {
        consumed += line.len();
        if line.trim_end() == DELIMITER {
            closed = true;
            break;
        }
        yaml.push_str(line);
    }
    if !closed {
        return Err(FrontmatterError::Unterminated);
    }

    let frontmatter = if yaml.trim().is_empty() {
        Map::new()
    } else {
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml)?;
        match yaml_to_json(value) {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            _ => return Err(FrontmatterError::NotAMapping),
        }
    };

    Ok(Document {
        frontmatter,
        body: normalised[consumed..].to_string(),
    })
}

/// Return only the Markdown body, with any YAML block removed.
pub fn strip(raw: &str) -> String {
    match parse(raw) {
        Ok(doc) => doc.body,
        // If the YAML is broken we still want to show the user their text.
        Err(_) => raw.replace("\r\n", "\n"),
    }
}

/// Serialise frontmatter + body back into a Markdown string.
///
/// If the frontmatter map is empty no `---` block is written at all.
pub fn render(frontmatter: &Map<String, Value>, body: &str) -> Result<String, FrontmatterError> {
    let body = body.replace("\r\n", "\n");
    if frontmatter.is_empty() {
        return Ok(body);
    }
    let yaml_value = json_to_yaml(Value::Object(frontmatter.clone()));
    let yaml = serde_yaml_ng::to_string(&yaml_value)?;
    let mut out = String::with_capacity(yaml.len() + body.len() + 16);
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(yaml.trim_end_matches('\n'));
    out.push('\n');
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(&body);
    Ok(out)
}

fn yaml_to_json(value: serde_yaml_ng::Value) -> Value {
    use serde_yaml_ng::Value as Y;
    match value {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::from(i)
            } else if let Some(u) = n.as_u64() {
                Value::from(u)
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        Y::String(s) => Value::String(s),
        Y::Sequence(seq) => Value::Array(seq.into_iter().map(yaml_to_json).collect()),
        Y::Mapping(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let key = match k {
                    Y::String(s) => s,
                    Y::Number(n) => n.to_string(),
                    Y::Bool(b) => b.to_string(),
                    Y::Null => "null".to_string(),
                    other => serde_yaml_ng::to_string(&other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                out.insert(key, yaml_to_json(v));
            }
            Value::Object(out)
        }
        Y::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

fn json_to_yaml(value: Value) -> serde_yaml_ng::Value {
    use serde_yaml_ng::Value as Y;
    match value {
        Value::Null => Y::Null,
        Value::Bool(b) => Y::Bool(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Y::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Y::Number(u.into())
            } else {
                Y::Number(n.as_f64().unwrap_or(0.0).into())
            }
        }
        Value::String(s) => Y::String(s),
        Value::Array(items) => Y::Sequence(items.into_iter().map(json_to_yaml).collect()),
        Value::Object(map) => {
            let mut out = serde_yaml_ng::Mapping::new();
            for (k, v) in map {
                out.insert(Y::String(k), json_to_yaml(v));
            }
            Y::Mapping(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_frontmatter_and_body() {
        let raw = "---\ntitle: GitHub\ntags:\n  - dev\n  - git\n---\nSome notes\n";
        let doc = parse(raw).unwrap();
        assert_eq!(doc.frontmatter["title"], json!("GitHub"));
        assert_eq!(doc.frontmatter["tags"], json!(["dev", "git"]));
        assert_eq!(doc.body, "Some notes\n");
    }

    #[test]
    fn no_frontmatter_is_fine() {
        let doc = parse("# Hello\n\n- [ ] task\n").unwrap();
        assert!(doc.frontmatter.is_empty());
        assert_eq!(doc.body, "# Hello\n\n- [ ] task\n");
    }

    #[test]
    fn handles_crlf_and_bom() {
        let raw = "\u{feff}---\r\ntitle: X\r\n---\r\nbody\r\n";
        let doc = parse(raw).unwrap();
        assert_eq!(doc.frontmatter["title"], json!("X"));
        assert_eq!(doc.body, "body\n");
    }

    #[test]
    fn unterminated_block_is_an_error() {
        assert!(matches!(
            parse("---\ntitle: X\nbody").unwrap_err(),
            FrontmatterError::Unterminated
        ));
    }

    #[test]
    fn strip_removes_yaml() {
        assert_eq!(strip("---\na: 1\n---\nhello"), "hello");
        assert_eq!(strip("plain"), "plain");
    }

    #[test]
    fn render_roundtrips() {
        let mut fm = Map::new();
        fm.insert("title".into(), json!("Bank"));
        fm.insert("secure_blob".into(), json!("ZYK1abc="));
        fm.insert("tags".into(), json!(["finance"]));
        let text = render(&fm, "notes here\n").unwrap();
        assert!(text.starts_with("---\n"));
        let back = parse(&text).unwrap();
        assert_eq!(back.frontmatter, fm);
        assert_eq!(back.body, "notes here\n");
    }

    #[test]
    fn render_without_frontmatter_is_plain_body() {
        assert_eq!(render(&Map::new(), "x").unwrap(), "x");
    }
}
