//! TUI colours. A theme is a small TOML file, the same format as Tapedeck's: two Catppuccin flavours
//! are built in, and `<config dir>/carrel/themes/<name>.toml` adds or overrides one. `theme = "<name>"`
//! in config.toml picks it.

use anyhow::{Context, Result, bail};
use ratatui::style::Color;
use serde::Deserialize;
use std::path::Path;

pub const DEFAULT: &str = "catppuccin-mocha";

const BUILTIN: &[(&str, &str)] = &[
    ("catppuccin-mocha", include_str!("themes/catppuccin-mocha.toml")),
    ("catppuccin-latte", include_str!("themes/catppuccin-latte.toml")),
];

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub border: Color,
    pub title: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    /// A paper whose text an AI can read.
    pub ready: Color,
    /// A PDF whose text couldn't be extracted.
    pub failed: Color,
    /// No PDF yet.
    pub wanted: Color,
    pub warning: Color,
    /// The book spines in the logo, any number.
    pub stripes: Vec<Color>,
}

impl Theme {
    /// The theme config.toml asks for, from `<config dir>/carrel/themes/` or built in.
    pub fn configured() -> Result<Theme> {
        let name = carrel::Config::load()?.theme.unwrap_or_else(|| DEFAULT.into());
        let themes = carrel::Config::path().and_then(|p| p.parent().map(|d| d.join("themes"))).unwrap_or_default();
        Self::load(&themes, &name)
    }

    /// A user file under `themes_dir` wins over a built-in of the same name.
    pub fn load(themes_dir: &Path, name: &str) -> Result<Theme> {
        let text = match std::fs::read_to_string(themes_dir.join(format!("{name}.toml"))) {
            Ok(text) => text,
            Err(_) => match BUILTIN.iter().find(|(n, _)| *n == name) {
                Some((_, text)) => text.to_string(),
                None => {
                    let mut known: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
                    let files = std::fs::read_dir(themes_dir).into_iter().flatten().flatten();
                    known.extend(
                        files.filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".toml").map(String::from)),
                    );
                    bail!("no theme {name:?}: not in {} and not built in ({})", themes_dir.display(), known.join(", "));
                }
            },
        };
        Self::parse(&text).with_context(|| format!("theme {name:?}"))
    }

    pub fn parse(text: &str) -> Result<Theme> {
        let t: Theme = toml::from_str(text).context("parsing the theme")?;
        if t.stripes.is_empty() {
            bail!("a theme needs at least one stripe colour");
        }
        Ok(t)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::parse(BUILTIN[0].1).expect("built-in theme parses")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_a_user_file_wins() {
        for (name, text) in BUILTIN {
            Theme::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        let dir = std::env::temp_dir().join(format!("carrel-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mine = BUILTIN[0].1.replace("Catppuccin Mocha", "Mine");
        std::fs::write(dir.join("catppuccin-mocha.toml"), mine).unwrap();
        assert_eq!(Theme::load(&dir, "catppuccin-mocha").unwrap().name, "Mine");
        assert_eq!(Theme::load(&dir, "catppuccin-latte").unwrap().name, "Catppuccin Latte");
        let err = Theme::load(&dir, "nope").unwrap_err().to_string();
        assert!(err.contains("catppuccin-latte") && err.contains("catppuccin-mocha"), "{err}");
        let tapedeck = "name = \"T\"\ntext = \"#ffffff\"\nmuted = \"gray\"\naccent = \"blue\"\nborder = \"gray\"\ntitle = \"blue\"\nselection_bg = \"black\"\nselection_fg = \"white\"\nready = \"green\"\nfailed = \"red\"\nwanted = \"yellow\"\nwarning = \"lightred\"\nstripes = [\"red\", \"yellow\", \"green\"]\n";
        assert_eq!(Theme::parse(tapedeck).unwrap().stripes.len(), 3, "Tapedeck themes work");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
