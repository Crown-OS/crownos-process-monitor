use std::borrow::Cow;
use std::fmt::Write;

use super::details::ProcessDetails;
use fields::Fields;

mod describe;
mod fields;
mod sections;

/// Whether an export may include the environment, which can hold secrets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnvironmentExport {
    #[default]
    Exclude,
    Include,
}

impl ProcessDetails {
    /// Plain text for the clipboard, one `name: value` line per field.
    pub fn to_text(&self, environment: EnvironmentExport) -> String {
        let mut text = String::new();
        let mut section = "";
        for field in self.fields(environment) {
            if field.section != section {
                section = field.section;
                let separator = if text.is_empty() { "" } else { "\n" };
                let _ = writeln!(text, "{separator}{section}");
            }
            let _ = writeln!(text, "  {}: {}", field.name, field.value);
        }
        text
    }

    /// `section,field,value` rows with RFC 4180 quoting.
    pub fn to_csv(&self, environment: EnvironmentExport) -> String {
        let mut csv = String::from("section,field,value\n");
        for field in self.fields(environment) {
            let _ = writeln!(
                csv,
                "{},{},{}",
                quote(field.section),
                quote(&field.name),
                quote(&field.value)
            );
        }
        csv
    }

    fn fields(&self, environment: EnvironmentExport) -> Vec<fields::Field<'_>> {
        let mut fields = Fields::default();
        sections::identity(&mut fields, &self.identity);
        sections::executable(&mut fields, self);
        sections::security(&mut fields, &self.security);
        sections::startup(&mut fields, &self.startup);
        if let Some(entry) = &self.desktop_entry {
            sections::desktop_entry(&mut fields, entry);
        }
        sections::resources(&mut fields, &self.resources);
        sections::children(&mut fields, &self.children);
        if environment == EnvironmentExport::Include {
            sections::environment(&mut fields, self.environment.as_deref().unwrap_or_default());
        }
        fields.into_list()
    }
}

fn quote(value: &str) -> Cow<'_, str> {
    if value.contains([',', '"', '\n', '\r']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
}

#[cfg(test)]
mod tests {
    use monitor_core::{Pid, ProcessKey, SourceResult, Uid};

    use super::*;
    use crate::apps::resolver::AppResolver;
    use crate::inspect::gather::{InspectContext, inspect_with};

    fn own_details() -> SourceResult<ProcessDetails> {
        let pid = Pid::new(std::process::id());
        let text = std::fs::read(format!("/proc/{pid}/stat"))
            .map_err(|error| monitor_core::SourceError::io("stat", error))?;
        let (_, stat) =
            crate::process::parse_stat(&text).ok_or(monitor_core::SourceError::parse("stat"))?;
        let resolver = AppResolver::new(
            crate::apps::desktop::DesktopIndex::default(),
            Uid::current(),
        );
        inspect_with(
            ProcessKey {
                pid,
                start_ticks: stat.start_ticks,
            },
            &InspectContext {
                app: None,
                resolver: &resolver,
            },
        )
    }

    #[test]
    fn quotes_csv_values() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("a,b"), "\"a,b\"");
        assert_eq!(quote("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn exports_without_environment_by_default() -> SourceResult<()> {
        let mut details = own_details()?;
        details.environment = Some(vec![("SECRET_TOKEN".to_owned(), "hunter2".to_owned())]);
        let text = details.to_text(EnvironmentExport::Exclude);
        assert!(text.starts_with("Identity\n"));
        assert!(text.contains("PID: "));
        assert!(!text.contains("hunter2"));
        let csv = details.to_csv(EnvironmentExport::Exclude);
        assert!(csv.starts_with("section,field,value\n"));
        assert!(!csv.contains("SECRET_TOKEN"));
        assert!(
            details
                .to_csv(EnvironmentExport::Include)
                .contains("Environment,SECRET_TOKEN,hunter2")
        );
        Ok(())
    }
}
