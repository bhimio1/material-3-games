//! Valve's KeyValues text format (`.vdf`, `.acf`): quoted keys and values,
//! nested `{ }` blocks, `//` comments. Lookups are case-insensitive because
//! Steam writes both `Valve` and `valve`.

#[derive(Clone, Debug, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    /// Follow a path of keys.
    pub fn path(&self, keys: &[&str]) -> Option<&Vdf> {
        keys.iter().try_fold(self, |v, k| v.get(k))
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Vdf::str)
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(e) => e,
            Vdf::Str(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        '"' => break,
                        other => s.push(other),
                    }
                }
                tokens.push(Token::Str(s));
            }
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            _ => {
                // Unquoted token (rare, but allowed).
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                tokens.push(Token::Str(s));
            }
        }
    }
    tokens
}

fn parse_map(tokens: &[Token], pos: &mut usize) -> Vec<(String, Vdf)> {
    let mut out = Vec::new();
    while *pos < tokens.len() {
        match &tokens[*pos] {
            Token::Close => {
                *pos += 1;
                break;
            }
            Token::Open => {
                // Stray brace: skip its contents.
                *pos += 1;
                parse_map(tokens, pos);
            }
            Token::Str(key) => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Token::Str(v)) => {
                        out.push((key.clone(), Vdf::Str(v.clone())));
                        *pos += 1;
                    }
                    Some(Token::Open) => {
                        *pos += 1;
                        let inner = parse_map(tokens, pos);
                        out.push((key.clone(), Vdf::Map(inner)));
                    }
                    Some(Token::Close) | None => break,
                }
            }
        }
    }
    out
}

/// Parse a whole document. Never fails: malformed input yields whatever
/// could be read.
pub fn parse(text: &str) -> Vdf {
    let tokens = tokenize(text);
    let mut pos = 0;
    Vdf::Map(parse_map(&tokens, &mut pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_folders() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/u/.local/share/Steam"
		"apps"
		{
			"228980"		"254370587"
			"570"		"39000000000"
		}
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
		"apps" { }
	}
}"#;
        let v = parse(text);
        let lf = v.get("LibraryFolders").unwrap();
        assert_eq!(lf.entries().len(), 2);
        assert_eq!(lf.path(&["1", "path"]).and_then(Vdf::str), Some("/mnt/games/SteamLibrary"));
        assert_eq!(lf.path(&["0", "apps"]).unwrap().entries().len(), 2);
    }

    #[test]
    fn escapes_comments_and_garbage() {
        let v = parse("// header\n\"a\" \"x\\\"y\\\\z\"\n\"b\" { \"c\" \"1\" ");
        assert_eq!(v.get_str("a"), Some("x\"y\\z"));
        assert_eq!(v.path(&["b", "c"]).and_then(Vdf::str), Some("1"));
        assert_eq!(parse("}}{{\"").entries().len(), 0);
    }
}
