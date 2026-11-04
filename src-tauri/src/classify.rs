//! What kind of thing was copied, and whether it looks like a secret that shouldn't be kept.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Link,
    Email,
    Color,
    Number,
    Code,
    Path,
    Image,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Link => "link",
            Kind::Email => "email",
            Kind::Color => "color",
            Kind::Number => "number",
            Kind::Code => "code",
            Kind::Path => "path",
            Kind::Image => "image",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "text" => Kind::Text,
            "link" => Kind::Link,
            "email" => Kind::Email,
            "color" => Kind::Color,
            "number" => Kind::Number,
            "code" => Kind::Code,
            "path" => Kind::Path,
            "image" => Kind::Image,
            _ => return None,
        })
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

static LINK: OnceLock<Regex> = OnceLock::new();
static BARE_LINK: OnceLock<Regex> = OnceLock::new();
static EMAIL: OnceLock<Regex> = OnceLock::new();
static COLOR: OnceLock<Regex> = OnceLock::new();
static NUMBER: OnceLock<Regex> = OnceLock::new();
static WIN_PATH: OnceLock<Regex> = OnceLock::new();
static UNIX_PATH: OnceLock<Regex> = OnceLock::new();

/// Classifies copied text. Only single-item copies count as links, emails, colours and numbers;
/// anything multi-line is code or text.
pub fn classify(text: &str) -> Kind {
    let t = text.trim();
    if t.is_empty() {
        return Kind::Text;
    }
    let single = !t.contains('\n') && t.len() < 2048;
    if single {
        if re(&LINK, r"^(?i)(https?|ftp)://[^\s]+$").is_match(t) || re(&BARE_LINK, r"^(?i)www\.[a-z0-9-]+(\.[a-z0-9-]+)+(/\S*)?$").is_match(t) {
            return Kind::Link;
        }
        if re(&EMAIL, r"^(?i)(mailto:)?[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}$").is_match(t) {
            return Kind::Email;
        }
        if re(&COLOR, r"^(?i)(#[0-9a-f]{3}|#[0-9a-f]{4}|#[0-9a-f]{6}|#[0-9a-f]{8}|rgba?\(\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*(,\s*[\d.]+\s*)?\)|hsla?\(\s*\d{1,3}(deg)?\s*,\s*\d{1,3}%\s*,\s*\d{1,3}%\s*(,\s*[\d.]+\s*)?\))$").is_match(t) {
            return Kind::Color;
        }
        if re(&NUMBER, r"^[-+]?[$€£₦¥]?\s?\d[\d,\s]*(\.\d+)?\s?%?$").is_match(t) {
            return Kind::Number;
        }
        if re(&WIN_PATH, r#"^(?i)([a-z]:\\|\\\\)[^<>"|?*\n]+$"#).is_match(t) || re(&UNIX_PATH, r"^(~|/)[^\s]*(/[^\s]+)+$").is_match(t) {
            return Kind::Path;
        }
    }
    if looks_like_code(t) {
        return Kind::Code;
    }
    Kind::Text
}

/// A rough guess: several lines with indentation, braces or semicolons at line ends, or common
/// keywords at line starts.
pub fn looks_like_code(t: &str) -> bool {
    let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return false;
    }
    let mut score = 0usize;
    for l in &lines {
        let s = l.trim_end();
        let lead = s.trim_start();
        if s.ends_with('{') || s.ends_with('}') || s.ends_with(';') || s.ends_with("=>") || s.ends_with(':') && lead.starts_with("def ") {
            score += 2;
        }
        if l.starts_with("    ") || l.starts_with('\t') {
            score += 1;
        }
        for kw in ["fn ", "function ", "const ", "let ", "var ", "def ", "class ", "import ", "export ", "return ", "if (", "for (", "#include", "public ", "SELECT ", "<div", "</", "pub fn", "async ", "package "] {
            if lead.starts_with(kw) {
                score += 2;
                break;
            }
        }
    }
    if lines.len() == 1 {
        return score >= 4;
    }
    score * 2 >= lines.len() * 2 + 2
}

/// Luhn check for card numbers.
pub fn luhn(digits: &str) -> bool {
    let ds: Vec<u32> = digits.chars().filter_map(|c| c.to_digit(10)).collect();
    if ds.len() < 13 || ds.len() > 19 {
        return false;
    }
    let mut sum = 0;
    for (i, d) in ds.iter().rev().enumerate() {
        let mut v = *d;
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum % 10 == 0
}

static CARD: OnceLock<Regex> = OnceLock::new();
static SECRETS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();

/// If the text looks like a password-grade secret, says what kind. Checked before anything is saved.
pub fn secret_kind(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.is_empty() || t.len() > 20_000 {
        return None;
    }
    let list = SECRETS.get_or_init(|| {
        [
            ("a private key", r"-----BEGIN ([A-Z0-9]+ )*PRIVATE KEY-----"),
            ("an AWS access key", r"\b(AKIA|ASIA)[0-9A-Z]{16}\b"),
            ("a GitHub token", r"\b(gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{60,})\b"),
            ("an API key", r"\bsk-(ant-|proj-)?[A-Za-z0-9_-]{20,}\b"),
            ("a Stripe key", r"\b(sk|rk)_live_[A-Za-z0-9]{20,}\b"),
            ("a Slack token", r"\bxox[abprs]-[A-Za-z0-9-]{10,}\b"),
            ("a Google API key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
            ("a login token", r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b"),
        ]
        .into_iter()
        .map(|(k, p)| (k, Regex::new(p).expect("valid regex")))
        .collect()
    });
    for (kind, r) in list {
        if r.is_match(t) {
            return Some(kind);
        }
    }
    // A card number on its own (spaces or dashes allowed) that passes the Luhn check.
    if re(&CARD, r"^\d{4}[ -]?\d{4}[ -]?\d{4}[ -]?\d{1,7}$").is_match(t) && luhn(t) {
        return Some("a card number");
    }
    None
}

/// A one-line label for lists: the first non-empty line, squeezed and cut to a sensible length.
pub fn preview(text: &str, max: usize) -> String {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let squeezed: String = first.split_whitespace().collect::<Vec<_>>().join(" ");
    if squeezed.chars().count() > max {
        let cut: String = squeezed.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        squeezed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(classify("https://example.com/a?b=1"), Kind::Link);
        assert_eq!(classify("  www.example.ng/menu "), Kind::Link);
        assert_eq!(classify("ada@example.com"), Kind::Email);
        assert_eq!(classify("mailto:ada@example.com"), Kind::Email);
        assert_eq!(classify("#FFE45C"), Kind::Color);
        assert_eq!(classify("rgb(255, 228, 92)"), Kind::Color);
        assert_eq!(classify("hsl(52deg, 100%, 68%)"), Kind::Color);
        assert_eq!(classify("₦12,500.00"), Kind::Number);
        assert_eq!(classify("42%"), Kind::Number);
        assert_eq!(classify(r"C:\Users\Ada\Desktop\notes.txt"), Kind::Path);
        assert_eq!(classify("/home/ada/projects/clipnest"), Kind::Path);
        assert_eq!(classify("~/Downloads/report.pdf"), Kind::Path);
        assert_eq!(classify("fn main() {\n    println!(\"hi\");\n}"), Kind::Code);
        assert_eq!(classify("const x = 1;\nconst y = 2;\nexport default x + y;"), Kind::Code);
        assert_eq!(classify("SELECT * FROM clips WHERE pinned = 1;"), Kind::Code);
        assert_eq!(classify("Meeting moved to Thursday.\nBring the slides."), Kind::Text);
        assert_eq!(classify("hello world"), Kind::Text);
        assert_eq!(classify("see https://example.com for details"), Kind::Text);
        assert_eq!(classify(""), Kind::Text);
    }

    #[test]
    fn secrets() {
        assert_eq!(secret_kind("4111 1111 1111 1111"), Some("a card number"));
        assert_eq!(secret_kind("4111 1111 1111 1112"), None);
        assert_eq!(secret_kind("AKIAIOSFODNN7EXAMPLE"), Some("an AWS access key"));
        assert_eq!(secret_kind("token: ghp_abcdefghijklmnopqrstuvwxyzABCDEFGHIJ"), Some("a GitHub token"));
        assert_eq!(secret_kind("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123"), Some("an API key"));
        assert_eq!(secret_kind("-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----"), Some("a private key"));
        assert_eq!(secret_kind("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"), Some("a login token"));
        assert_eq!(secret_kind("Call me on 0803 000 0000"), None);
        assert_eq!(secret_kind("12345"), None);
        assert!(luhn("4111-1111-1111-1111"));
        assert!(!luhn("79927398713"), "too short for a card");
    }

    #[test]
    fn previews() {
        assert_eq!(preview("\n\n  Hello   world  \nsecond", 40), "Hello world");
        assert_eq!(preview("abcdefghij", 5), "abcd…");
        assert_eq!(preview("", 10), "");
    }
}
