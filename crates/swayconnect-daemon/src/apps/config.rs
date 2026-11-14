//! Config sections `[apps]` and `[launcher]` (`docs/DESIGN.md` 11.9). `config.rs` embeds
//! both with `#[serde(default)]`.

use serde::Deserialize;

use crate::proc::{ProgramArgv, ProgramPath};

const DEFAULT_SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const DEFAULT_RISKY: [&str; 2] = ["gparted", "org.cachyos.KernelManager"];
const DEFAULT_TERMINAL: [&str; 2] = ["/usr/bin/alacritty", "-e"];
const DEFAULT_LAUNCHER: &str = "/usr/bin/fuzzel";

/// `[apps]`: how entries start, which are hidden and which need `launch.risky`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppsConfig {
    pub systemd_run: ProgramPath,
    /// Desktop entry ids left out of topic `apps`.
    pub deny: Vec<String>,
    /// Desktop entry ids that need `launch.risky`. Terminal entries always do.
    pub risky: Vec<String>,
    /// Program and arguments that run a `Terminal=true` entry.
    pub terminal: ProgramArgv,
}

impl Default for AppsConfig {
    fn default() -> AppsConfig {
        AppsConfig {
            systemd_run: ProgramPath::new(DEFAULT_SYSTEMD_RUN.into())
                .expect("the default path is absolute"),
            deny: Vec::new(),
            risky: DEFAULT_RISKY.map(str::to_owned).to_vec(),
            terminal: argv(&DEFAULT_TERMINAL),
        }
    }
}

/// `[launcher]`: the program `launcher.open` runs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LauncherConfig {
    pub argv: ProgramArgv,
}

impl Default for LauncherConfig {
    fn default() -> LauncherConfig {
        LauncherConfig {
            argv: argv(&[DEFAULT_LAUNCHER]),
        }
    }
}

fn argv(words: &[&str]) -> ProgramArgv {
    let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    ProgramArgv::try_from(words).expect("the default argv is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Sections {
        #[serde(default)]
        apps: AppsConfig,
        #[serde(default)]
        launcher: LauncherConfig,
    }

    #[test]
    fn defaults_match_the_design() {
        let sections: Sections = toml::from_str("").expect("empty config");
        assert_eq!(
            sections.apps.systemd_run.as_path().to_str(),
            Some(DEFAULT_SYSTEMD_RUN)
        );
        assert_eq!(sections.apps.risky, DEFAULT_RISKY);
        assert!(sections.apps.deny.is_empty());
        assert_eq!(
            sections.apps.terminal.program().as_path().to_str(),
            Some("/usr/bin/alacritty")
        );
        assert_eq!(sections.apps.terminal.args(), ["-e"]);
        assert_eq!(
            sections.launcher.argv.program().as_path().to_str(),
            Some(DEFAULT_LAUNCHER)
        );
    }

    #[test]
    fn values_are_read_and_bad_ones_refused() {
        let text = "[apps]\nsystemd_run = \"/opt/systemd-run\"\ndeny = [\"vim\"]\n\
                    risky = []\nterminal = [\"/opt/foot\"]\n[launcher]\nargv = [\"/opt/tofi\", \"--x\"]\n";
        let sections: Sections = toml::from_str(text).expect("valid config");
        assert_eq!(sections.apps.deny, ["vim"]);
        assert!(sections.apps.risky.is_empty());
        assert!(sections.apps.terminal.args().is_empty());
        assert_eq!(sections.launcher.argv.args(), ["--x"]);

        for bad in [
            "[apps]\nsystemd_run = \"systemd-run\"\n",
            "[apps]\nterminal = []\n",
            "[apps]\nshell = \"/bin/sh\"\n",
            "[launcher]\nargv = [\"fuzzel\"]\n",
        ] {
            assert!(toml::from_str::<Sections>(bad).is_err(), "{bad:?}");
        }
    }
}
