//! Editing one value in a text KeyValues file (`config.vdf`,
//! `localconfig.vdf`) without rewriting the rest: everything outside the
//! edited value keeps its exact bytes, comments and ordering.

#[derive(Debug)]
enum Tok {
    /// Quoted or bare string: (start, end) of the whole token, decoded text.
    Str(usize, usize, String),
    Open,
    Close(usize),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                let start = i;
                i += 1;
                let mut s = Vec::new();
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        i += 1;
                        s.push(match b[i] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            c => c,
                        });
                    } else {
                        s.push(b[i]);
                    }
                    i += 1;
                }
                i = (i + 1).min(b.len());
                out.push(Tok::Str(start, i, String::from_utf8_lossy(&s).into_owned()));
            }
            b'{' => {
                out.push(Tok::Open);
                i += 1;
            }
            b'}' => {
                out.push(Tok::Close(i));
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'{' | b'}' | b'"') {
                    i += 1;
                }
                out.push(Tok::Str(start, i, text[start..i].to_string()));
            }
        }
    }
    out
}

#[derive(Debug)]
enum Val {
    Str(usize, usize),
    Block { close: usize, children: Vec<Node> },
}

#[derive(Debug)]
struct Node {
    key: String,
    val: Val,
}

fn build(toks: &[Tok], pos: &mut usize, end_of_text: usize) -> (Vec<Node>, usize) {
    let mut nodes = Vec::new();
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Close(at) => {
                *pos += 1;
                return (nodes, *at);
            }
            Tok::Open => {
                *pos += 1;
                build(toks, pos, end_of_text);
            }
            Tok::Str(_, _, key) => {
                let key = key.clone();
                *pos += 1;
                match toks.get(*pos) {
                    Some(Tok::Str(s, e, _)) => {
                        nodes.push(Node { key, val: Val::Str(*s, *e) });
                        *pos += 1;
                    }
                    Some(Tok::Open) => {
                        *pos += 1;
                        let (children, close) = build(toks, pos, end_of_text);
                        nodes.push(Node { key, val: Val::Block { close, children } });
                    }
                    _ => break,
                }
            }
        }
    }
    (nodes, end_of_text)
}

pub fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Read a string value at `path` (case-insensitive keys).
pub fn get(text: &str, path: &[&str]) -> Option<String> {
    let doc = crate::library::vdf::parse(text);
    doc.path(path).and_then(|v| v.str()).map(str::to_string)
}

fn line_indent(text: &str, pos: usize) -> String {
    let start = text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[start..].chars().take_while(|c| *c == '\t' || *c == ' ').collect()
}

/// Set the string at `path`, creating missing blocks. The rest of the text
/// is untouched.
pub fn set(text: &str, path: &[&str], value: &str) -> String {
    assert!(!path.is_empty(), "empty path");
    let toks = tokenize(text);
    let mut pos = 0;
    let (root, _) = build(&toks, &mut pos, text.len());

    // Walk as deep as the existing structure goes.
    let mut level: &[Node] = &root;
    let mut close: Option<usize> = None; // None = document level
    let mut depth = 0;
    for (i, key) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        let found = level.iter().rev().find(|n| n.key.eq_ignore_ascii_case(key));
        match (found, last) {
            (Some(Node { val: Val::Str(s, e), .. }), true) => {
                return format!("{}{}{}", &text[..*s], quote(value), &text[*e..]);
            }
            (Some(Node { val: Val::Block { close: c, children }, .. }), false) => {
                level = children;
                close = Some(*c);
                depth = i + 1;
            }
            _ => break,
        }
    }

    // Build the missing part, indented one level deeper than its parent.
    let (insert_at, base) = match close {
        Some(c) => (c, line_indent(text, c)),
        None => (text.len(), String::new()),
    };
    let inner = if close.is_some() { format!("{base}\t") } else { String::new() };
    let mut s = String::new();
    let rest = &path[depth..];
    for (j, key) in rest.iter().enumerate() {
        let ind = format!("{inner}{}", "\t".repeat(j));
        if j + 1 == rest.len() {
            s.push_str(&format!("{ind}{}\t\t{}\n", quote(key), quote(value)));
        } else {
            s.push_str(&format!("{ind}{}\n{ind}{{\n", quote(key)));
        }
    }
    for j in (0..rest.len().saturating_sub(1)).rev() {
        s.push_str(&format!("{inner}{}}}\n", "\t".repeat(j)));
    }
    match close {
        Some(c) => {
            // Insert on its own line just before the closing brace's line.
            let line_start = text[..c].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if text[line_start..c].trim().is_empty() {
                format!("{}{}{}", &text[..line_start], s, &text[line_start..])
            } else {
                format!("{}\n{}{}{}", &text[..c], s, base, &text[c..])
            }
        }
        None => {
            let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
            format!("{}{sep}{s}", &text[..insert_at])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"CompatToolMapping\"\n\t\t\t\t{\n\t\t\t\t\t\"570\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"name\"\t\t\"proton_9\"\n\t\t\t\t\t\t\"config\"\t\t\"\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t\t// keep me\n\t\t\t\t\"Other\"\t\t\"x\"\n\t\t\t}\n\t\t}\n\t}\n}\n";

    const MAP: [&str; 7] = ["InstallConfigStore", "Software", "Valve", "Steam", "CompatToolMapping", "570", "name"];

    #[test]
    fn replaces_in_place() {
        let t = set(CONFIG, &MAP, "GE-Proton9-20");
        assert_eq!(get(&t, &MAP).as_deref(), Some("GE-Proton9-20"));
        assert_eq!(t.replace("GE-Proton9-20", "proton_9"), CONFIG);
    }

    #[test]
    fn creates_missing_blocks() {
        let mut path = MAP;
        path[5] = "1091500";
        let t = set(CONFIG, &path, "proton_experimental");
        assert_eq!(get(&t, &path).as_deref(), Some("proton_experimental"));
        assert_eq!(get(&t, &MAP).as_deref(), Some("proton_9"));
        assert!(t.contains("// keep me"));
        assert_eq!(get(&t, &["InstallConfigStore", "Software", "Valve", "Steam", "Other"]).as_deref(), Some("x"));
        // Balanced braces.
        assert_eq!(t.matches('{').count(), t.matches('}').count());
    }

    #[test]
    fn quoting_and_empty_docs() {
        let t = set("", &["a", "b"], "say \"hi\" \\o/");
        assert_eq!(get(&t, &["a", "b"]).as_deref(), Some("say \"hi\" \\o/"));
        let t2 = set(&t, &["a", "c"], "%command% -novid");
        assert_eq!(get(&t2, &["a", "b"]).as_deref(), Some("say \"hi\" \\o/"));
        assert_eq!(get(&t2, &["a", "c"]).as_deref(), Some("%command% -novid"));
    }

    #[test]
    fn launch_options_in_localconfig() {
        let lc = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"570\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1700000000\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
        let p = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps", "570", "LaunchOptions"];
        let t = set(lc, &p, "gamemoderun %command%");
        assert_eq!(get(&t, &p).as_deref(), Some("gamemoderun %command%"));
        assert_eq!(get(&t, &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps", "570", "LastPlayed"]).as_deref(), Some("1700000000"));
        assert!(t.contains("\t\t\t\t\t\t\"LaunchOptions\"\t\t\"gamemoderun %command%\"\n\t\t\t\t\t}"));
    }
}
