use std::io::Write;
use std::path::{Path, PathBuf};

use monitor_alerts::QuietRule;

const FILE_NAME: &str = "quiet-rules";
const TEMPORARY_SUFFIX: &str = ".new";

/// The quiet rules, one [`QuietRule::to_line`] per line, kept next to the
/// history database (`$XDG_STATE_HOME/crownos-process-monitor/`).
pub(crate) struct QuietFile {
    path: PathBuf,
}

impl QuietFile {
    pub fn beside(history: &Path) -> Self {
        Self {
            path: history.with_file_name(FILE_NAME),
        }
    }

    /// Unreadable or malformed lines are skipped: a lost rule only means an
    /// alert shows again.
    pub fn load(&self) -> Vec<QuietRule> {
        std::fs::read_to_string(&self.path)
            .map(|text| text.lines().filter_map(QuietRule::parse_line).collect())
            .unwrap_or_default()
    }

    /// Replaces the file atomically; a failed write keeps the previous rules.
    pub fn save(&self, rules: &[QuietRule]) {
        let mut temporary = self.path.clone().into_os_string();
        temporary.push(TEMPORARY_SUFFIX);
        let temporary = PathBuf::from(temporary);
        let written = std::fs::File::create(&temporary).and_then(|mut file| {
            rules
                .iter()
                .try_for_each(|rule| writeln!(file, "{}", rule.to_line()))
        });
        if written.is_ok() {
            let _ = std::fs::rename(&temporary, &self.path);
        } else {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use monitor_alerts::{AlertKind, QuietScope, QuietUntil};

    use super::*;

    #[test]
    fn saves_and_loads_rules() {
        let directory =
            std::env::temp_dir().join(format!("monitor-engine-quiet-{}", std::process::id()));
        assert!(std::fs::create_dir_all(&directory).is_ok());
        let file = QuietFile::beside(&directory.join("history.sqlite"));
        assert!(file.load().is_empty());
        let rules = [QuietRule {
            scope: QuietScope::Kind(AlertKind::MachineHot),
            until: QuietUntil::Forever,
        }];
        file.save(&rules);
        assert_eq!(file.load(), rules);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
