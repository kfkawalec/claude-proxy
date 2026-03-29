#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Pl,
    En,
}

pub fn detect_lang() -> Lang {
    if let Ok(out) = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
        if let Some(first) = s
            .lines()
            .map(|l| l.trim().trim_matches(',').trim_matches('"'))
            .find(|l| l.starts_with("en") || l.starts_with("pl"))
        {
            return if first.starts_with("pl") { Lang::Pl } else { Lang::En };
        }
    }
    if std::env::var("LANGUAGE")
        .unwrap_or_default()
        .to_lowercase()
        .starts_with("pl")
    {
        Lang::Pl
    } else {
        Lang::En
    }
}

#[inline]
pub fn tr<'a>(lang: Lang, pl: &'a str, en: &'a str) -> &'a str {
    match lang {
        Lang::Pl => pl,
        Lang::En => en,
    }
}
