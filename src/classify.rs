//! Display-only helpers: what kind of text a clip looks like (code, link,
//! color…), simple syntax tokens, and time grouping/labels for the list.

use chrono::{DateTime, Local, TimeZone};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextKind {
    Plain,
    Code,
    Link,
    Color,
}

pub fn text_kind(s: &str) -> TextKind {
    let t = s.trim();
    if parse_hex_color(t).is_some() {
        return TextKind::Color;
    }
    if !t.contains(char::is_whitespace) && (t.starts_with("http://") || t.starts_with("https://")) {
        return TextKind::Link;
    }
    if looks_like_code(t) {
        return TextKind::Code;
    }
    TextKind::Plain
}

/// Commands that are unlikely to start an English sentence.
const SHELL: &[&str] = &[
    "ssh",
    "git",
    "cargo",
    "npm",
    "npx",
    "yarn",
    "pnpm",
    "brew",
    "cd",
    "ls",
    "sudo",
    "docker",
    "kubectl",
    "curl",
    "wget",
    "python3",
    "pip",
    "pip3",
    "mkdir",
    "chmod",
    "chown",
    "grep",
    "rustup",
    "scp",
    "rsync",
    "systemctl",
    "osascript",
    "rm",
    "mv",
    "cp",
];
const SQL: &[&str] = &[
    "select", "insert", "update", "delete", "create", "alter", "drop", "with",
];

fn looks_like_code(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    // a pasted terminal line may start with the prompt
    let t = t.strip_prefix("$ ").unwrap_or(t);
    let first_word = t.split_whitespace().next().unwrap_or("");
    let rest_has_args = t.split_whitespace().nth(1).is_some();
    let prose_end = t.ends_with(['.', '?', '!']);
    // shell commands are typed lower case: `git push`, `$ cargo run`
    if SHELL.contains(&first_word) && rest_has_args && !prose_end {
        return true;
    }
    // SQL keywords written in capitals: `SELECT …`
    let upper = first_word.chars().all(|c| c.is_ascii_uppercase());
    if upper && SQL.contains(&first_word.to_lowercase().as_str()) && rest_has_args {
        return true;
    }
    let is_json =
        (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'));
    if is_json && t.contains(':') {
        return true;
    }
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() == 1 {
        let l = lines[0].trim();
        return l.ends_with(';') && l.contains('(');
    }
    const SIGNALS: &[&str] = &[
        "=>", "->", "fn ", "def ", "function", "const ", "let ", "import ", "#include", "</",
        "return ", "pub ", "class ", "#!/", "::", "!=", "==",
    ];
    let code_lines = lines
        .iter()
        .filter(|l| {
            let trimmed = l.trim_end();
            trimmed.ends_with(';')
                || trimmed.ends_with('{')
                || trimmed.ends_with('}')
                || l.starts_with("    ")
                || l.starts_with('\t')
                || SIGNALS.iter().any(|s| l.contains(s))
        })
        .count();
    code_lines * 10 >= lines.len() * 4
}

// ---------- links ----------

pub struct Link<'a> {
    pub scheme: &'a str,
    pub domain: &'a str,
    pub path: &'a str,
}

pub fn parse_link(s: &str) -> Link<'_> {
    let s = s.trim();
    let (scheme, rest) = s.split_once("://").unwrap_or(("url", s));
    let (domain, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    Link {
        scheme,
        domain,
        path,
    }
}

// ---------- colors ----------

/// `#rgb`, `#rrggbb` or `#rrggbbaa` → (r, g, b).
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => {
            let d: Vec<u8> = hex
                .chars()
                .map(|c| c.to_digit(16).unwrap() as u8 * 17)
                .collect();
            Some([d[0], d[1], d[2]])
        }
        6 | 8 => Some([byte(0)?, byte(2)?, byte(4)?]),
        _ => None,
    }
}

/// "h°, s%, l%" as in CSS.
pub fn hsl_string([r, g, b]: [u8; 3]) -> String {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let l = (mx + mn) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if mx != mn {
        let d = mx - mn;
        s = if l > 0.5 {
            d / (2.0 - mx - mn)
        } else {
            d / (mx + mn)
        };
        h = if mx == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if mx == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } * 60.0;
    }
    format!(
        "{}°, {}%, {}%",
        h.round() as i32,
        (s * 100.0).round() as i32,
        (l * 100.0).round() as i32
    )
}

// ---------- code tokens ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Num,
    Flag,
    Path,
    Comment,
}

const KEYWORDS: &[&str] = &[
    // SQL (upper case only, so prose isn't lit up)
    "SELECT", "FROM", "WHERE", "GROUP", "BY", "ORDER", "LIMIT", "DESC", "ASC", "AND", "OR", "AS",
    "INTERVAL", "HOUR", "JOIN", "LEFT", "INNER", "ON", "INSERT", "INTO", "VALUES", "UPDATE", "SET",
    "DELETE", "CREATE", "TABLE", "NOT", "NULL", "WITH", "HAVING", "DISTINCT", "COUNT",
    // common languages
    "fn", "let", "mut", "pub", "use", "impl", "struct", "enum", "match", "if", "else", "for",
    "while", "loop", "return", "const", "function", "var", "import", "export", "from", "class",
    "def", "async", "await", "true", "false", "null", "None", "True", "False", "self", "new",
    // shell
    "ssh", "sudo", "git", "cd", "cargo", "npm", "docker", "kubectl", "curl", "export",
];

/// Split one line into coloured tokens (a deliberately small, language-agnostic highlighter).
pub fn tokenize(line: &str) -> Vec<(String, Tok)> {
    let mut out: Vec<(String, Tok)> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let push = |out: &mut Vec<(String, Tok)>, s: String, t: Tok| {
        if let Some(last) = out.last_mut() {
            if last.1 == t && t == Tok::Plain {
                last.0.push_str(&s);
                return;
            }
        }
        out.push((s, t));
    };
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().collect();
        // comments run to end of line
        if rest.starts_with("//")
            || (c == '#' && (i == 0 || chars[i - 1].is_whitespace()) && !rest.starts_with("#!"))
            || rest.starts_with("-- ")
        {
            push(&mut out, rest, Tok::Comment);
            break;
        }
        if c == '"' || c == '\'' || c == '`' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != c {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            push(&mut out, chars[i..end].iter().collect(), Tok::Str);
            i = end;
            continue;
        }
        if c.is_whitespace() || "(),;{}[]=:<>+*|&!".contains(c) {
            push(&mut out, c.to_string(), Tok::Plain);
            i += 1;
            continue;
        }
        // a "word": up to whitespace or punctuation
        let mut j = i;
        while j < chars.len() && !chars[j].is_whitespace() && !"(),;{}[]=:<>\"'`".contains(chars[j])
        {
            j += 1;
        }
        let word: String = chars[i..j].iter().collect();
        let tok = if KEYWORDS.contains(&word.as_str()) {
            Tok::Keyword
        } else if word.chars().next().is_some_and(|c| c.is_ascii_digit())
            && word
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '_')
        {
            Tok::Num
        } else if word.starts_with('-')
            && word.len() > 1
            && word[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '-')
        {
            Tok::Flag
        } else if word.starts_with('~') || (word.starts_with('/') && word.len() > 1) {
            Tok::Path
        } else {
            Tok::Plain
        };
        push(&mut out, word, tok);
        i = j.max(i + 1);
    }
    out
}

// ---------- time ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Pinned,
    Today,
    Yesterday,
    ThisWeek,
    Earlier,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Pinned => "Pinned",
            Group::Today => "Today",
            Group::Yesterday => "Yesterday",
            Group::ThisWeek => "This week",
            Group::Earlier => "Earlier",
        }
    }
}

fn local(ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Local::now)
}

/// Whole calendar days between `then` and `now` in local time.
fn days_between(then_ms: i64, now_ms: i64) -> i64 {
    (local(now_ms).date_naive() - local(then_ms).date_naive()).num_days()
}

pub fn group_for(created_ms: i64, pinned: bool, now_ms: i64) -> Group {
    if pinned {
        return Group::Pinned;
    }
    match days_between(created_ms, now_ms) {
        ..=0 => Group::Today,
        1 => Group::Yesterday,
        2..=6 => Group::ThisWeek,
        _ => Group::Earlier,
    }
}

pub fn ago(created_ms: i64, now_ms: i64) -> String {
    let secs = ((now_ms - created_ms) / 1000).max(0);
    match days_between(created_ms, now_ms) {
        ..=0 => match secs {
            0..=4 => "just now".into(),
            5..=59 => format!("{secs}s ago"),
            60..=3599 => format!("{}m ago", secs / 60),
            _ => format!("{}h ago", secs / 3600),
        },
        1 => "Yesterday".into(),
        d @ 2..=6 => format!("{d} days ago"),
        _ => local(created_ms).format("%b %-d").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(text_kind("#C8F169"), TextKind::Color);
        assert_eq!(text_kind(" #fff "), TextKind::Color);
        assert_eq!(text_kind("#zzz"), TextKind::Plain);
        assert_eq!(
            text_kind("https://docs.rs/eframe/latest/eframe/trait.App.html"),
            TextKind::Link
        );
        assert_eq!(text_kind("see https://x.com for more"), TextKind::Plain);
        assert_eq!(
            text_kind("ssh deploy@10.20.0.14 -i ~/.ssh/id_ed25519"),
            TextKind::Code
        );
        assert_eq!(
            text_kind("SELECT region, count() AS flows\nFROM flows\nLIMIT 20;"),
            TextKind::Code
        );
        assert_eq!(
            text_kind("fn main() {\n    println!(\"hi\");\n}"),
            TextKind::Code
        );
        assert_eq!(text_kind("{\"a\": 1}"), TextKind::Code);
        assert_eq!(text_kind("name@example.com"), TextKind::Plain);
        assert_eq!(
            text_kind("Thanks for the update.\n\nCould you share the plan?\n\nBest regards"),
            TextKind::Plain
        );
        assert_eq!(text_kind("select the best option"), TextKind::Plain);
        assert_eq!(text_kind("Select the best option"), TextKind::Plain);
        assert_eq!(text_kind("Open the door, please"), TextKind::Plain);
        assert_eq!(text_kind("git is great."), TextKind::Plain);
        assert_eq!(text_kind("$ cargo run --release"), TextKind::Code);
    }

    #[test]
    fn colors_and_links() {
        assert_eq!(parse_hex_color("#C8F169"), Some([200, 241, 105]));
        assert_eq!(parse_hex_color("#fff"), Some([255, 255, 255]));
        assert_eq!(hsl_string([200, 241, 105]), "78°, 83%, 68%");
        let l = parse_link("https://docs.rs/eframe/latest");
        assert_eq!(
            (l.scheme, l.domain, l.path),
            ("https", "docs.rs", "/eframe/latest")
        );
        assert_eq!(parse_link("https://example.com").path, "/");
    }

    #[test]
    fn tokens() {
        let t = tokenize("ssh deploy@10.20.0.14 -i ~/.ssh/key");
        assert_eq!(t[0], ("ssh".into(), Tok::Keyword));
        assert!(t.contains(&("-i".into(), Tok::Flag)));
        assert!(t.contains(&("~/.ssh/key".into(), Tok::Path)));
        let t = tokenize("LIMIT 20; -- top");
        assert!(t.contains(&("20".into(), Tok::Num)));
        assert_eq!(t.last().unwrap().1, Tok::Comment);
        let t = tokenize("let s = \"a b\";");
        assert!(t.contains(&("\"a b\"".into(), Tok::Str)));
        // tokens always reassemble to the original line
        let line = "x = f('q', 3) // done";
        assert_eq!(
            tokenize(line)
                .iter()
                .map(|t| t.0.as_str())
                .collect::<String>(),
            line
        );
    }

    #[test]
    fn time_groups() {
        let now = crate::store::now_ms();
        let day = 86_400_000;
        assert_eq!(group_for(now, true, now), Group::Pinned);
        assert_eq!(group_for(now - 1000, false, now), Group::Today);
        assert_eq!(group_for(now - day, false, now), Group::Yesterday);
        assert_eq!(group_for(now - 3 * day, false, now), Group::ThisWeek);
        assert_eq!(group_for(now - 30 * day, false, now), Group::Earlier);
        assert_eq!(ago(now, now), "just now");
        let two_min = ago(now - 120_000, now);
        assert!(two_min == "2m ago" || two_min == "Yesterday", "{two_min}");
        assert_eq!(ago(now - 3 * day, now), "3 days ago");
    }
}
