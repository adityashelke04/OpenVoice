//! Per-application formatting profiles.
//!
//! The same sentence should come out differently depending on where it lands.
//! `git status` typed into a terminal must not become `Git status.`, and a Slack
//! message should not arrive uncapitalized. Profiles encode that.

use serde::{Deserialize, Serialize};

/// How aggressively to remove disfluencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillerLevel {
    /// Leave the transcript alone.
    Off,
    /// Remove unambiguous disfluencies only: "um", "uh", "erm", "mhm".
    #[default]
    Light,
    /// Also remove discourse markers: "like", "you know", "I mean", "basically",
    /// "sort of", "kind of".
    ///
    /// Deliberately not the default. These words are sometimes load-bearing —
    /// "sort of" can be the actual meaning — and silently deleting them produces
    /// sentences the user did not say.
    Aggressive,
}

/// Sentence capitalization policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capitalization {
    /// Leave case exactly as the model produced it.
    Off,
    /// Capitalize the first letter of each sentence.
    #[default]
    Sentence,
    /// Force the first character lowercase. For shells, where `Ls` is not a command.
    ForceLower,
}

/// A named set of formatting rules bound to a set of executables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    /// Profile name, recorded in history.
    pub name: String,
    /// Executable names this profile applies to, matched case-insensitively.
    pub matches: Vec<String>,
    /// Capitalization policy.
    pub capitalize: Capitalization,
    /// Append a period if the text ends without terminal punctuation.
    pub end_period: bool,
    /// Filler removal level.
    pub fillers: FillerLevel,
    /// Whether spoken punctuation and layout commands are interpreted.
    pub voice_commands: bool,
    /// Whether "camel case ..." style identifier transforms are interpreted.
    pub case_transforms: bool,
    /// Dictionary group names to enable, e.g. `["code", "shell"]`.
    pub dictionaries: Vec<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "default".into(),
            matches: Vec::new(),
            capitalize: Capitalization::Sentence,
            end_period: false,
            fillers: FillerLevel::Light,
            voice_commands: true,
            case_transforms: true,
            dictionaries: vec!["code".into()],
        }
    }
}

impl Profile {
    /// Shell and terminal windows.
    ///
    /// No capitalization and no trailing period: both would corrupt a command.
    /// Case transforms stay on because flag and path names benefit from them.
    #[must_use]
    pub fn terminal() -> Self {
        Self {
            name: "terminal".into(),
            matches: vec![
                "WindowsTerminal.exe".into(),
                "powershell.exe".into(),
                "pwsh.exe".into(),
                "cmd.exe".into(),
                "wt.exe".into(),
                "alacritty.exe".into(),
            ],
            capitalize: Capitalization::ForceLower,
            end_period: false,
            fillers: FillerLevel::Light,
            voice_commands: true,
            case_transforms: true,
            dictionaries: vec!["shell".into(), "code".into()],
        }
    }

    /// Code editors and IDEs.
    #[must_use]
    pub fn editor() -> Self {
        Self {
            name: "editor".into(),
            matches: vec![
                "Code.exe".into(),
                "Cursor.exe".into(),
                "idea64.exe".into(),
                "devenv.exe".into(),
                "zed.exe".into(),
                "sublime_text.exe".into(),
            ],
            capitalize: Capitalization::Sentence,
            end_period: false,
            fillers: FillerLevel::Light,
            voice_commands: true,
            case_transforms: true,
            dictionaries: vec!["code".into()],
        }
    }

    /// Chat, mail, docs, and other prose surfaces.
    #[must_use]
    pub fn prose() -> Self {
        Self {
            name: "prose".into(),
            matches: vec![
                "slack.exe".into(),
                "Discord.exe".into(),
                "Notion.exe".into(),
                "chrome.exe".into(),
                "msedge.exe".into(),
                "firefox.exe".into(),
                "olk.exe".into(),
            ],
            capitalize: Capitalization::Sentence,
            end_period: true,
            fillers: FillerLevel::Aggressive,
            voice_commands: true,
            case_transforms: false,
            dictionaries: vec!["code".into()],
        }
    }

    /// The profiles shipped out of the box, in match precedence order.
    #[must_use]
    pub fn builtins() -> Vec<Self> {
        vec![Self::terminal(), Self::editor(), Self::prose()]
    }

    /// Whether this profile claims `exe`.
    #[must_use]
    pub fn claims(&self, exe: &str) -> bool {
        self.matches.iter().any(|m| m.eq_ignore_ascii_case(exe))
    }
}

/// Choose a profile for an executable, falling back to the default.
///
/// Order matters: the first profile in `profiles` that claims the executable wins,
/// so user-defined profiles must be placed ahead of the builtins by the caller.
#[must_use]
pub fn select<'a>(profiles: &'a [Profile], exe: &str) -> Option<&'a Profile> {
    profiles.iter().find(|p| p.claims(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_profile_never_corrupts_a_command() {
        let t = Profile::terminal();
        assert_eq!(t.capitalize, Capitalization::ForceLower);
        assert!(!t.end_period, "a trailing period would break the command");
    }

    #[test]
    fn selection_is_case_insensitive_on_exe_name() {
        let ps = Profile::builtins();
        assert_eq!(
            select(&ps, "code.exe").map(|p| p.name.as_str()),
            Some("editor")
        );
        assert_eq!(
            select(&ps, "CODE.EXE").map(|p| p.name.as_str()),
            Some("editor")
        );
    }

    #[test]
    fn unknown_executable_selects_nothing() {
        assert!(select(&Profile::builtins(), "notepad.exe").is_none());
    }

    #[test]
    fn first_match_wins_so_user_profiles_can_override() {
        let mut ps = vec![Profile {
            name: "my-vscode".into(),
            matches: vec!["Code.exe".into()],
            ..Profile::default()
        }];
        ps.extend(Profile::builtins());
        assert_eq!(
            select(&ps, "Code.exe").map(|p| p.name.as_str()),
            Some("my-vscode")
        );
    }
}
