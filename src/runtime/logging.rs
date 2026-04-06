use std::path::PathBuf;

use radroots_log::{LogFileLayout, LoggingOptions};

use crate::runtime::config::LoggingConfig;

const CLI_LOG_FILE_NAME: &str = "radroots-cli.log";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggingState {
    pub initialized: bool,
    pub current_file: Option<PathBuf>,
}

pub fn initialize_logging(config: &LoggingConfig) -> Result<LoggingState, radroots_log::Error> {
    let options = to_radroots_logging_options(config);
    let state = LoggingState {
        initialized: true,
        current_file: options.resolved_current_log_file_path(),
    };
    radroots_log::init_logging(options)?;
    Ok(state)
}

pub fn to_radroots_logging_options(config: &LoggingConfig) -> LoggingOptions {
    LoggingOptions {
        dir: config.directory.clone(),
        file_name: CLI_LOG_FILE_NAME.to_owned(),
        stdout: config.stdout,
        default_level: Some(config.filter.clone()),
        file_layout: LogFileLayout::PrefixedDate,
    }
}

#[cfg(test)]
mod tests {
    use super::to_radroots_logging_options;
    use crate::runtime::config::LoggingConfig;
    use std::path::PathBuf;

    #[test]
    fn logging_options_use_cli_file_name() {
        let options = to_radroots_logging_options(&LoggingConfig {
            filter: "info".to_owned(),
            directory: Some(PathBuf::from("logs")),
            stdout: false,
        });
        assert_eq!(options.file_name, "radroots-cli.log");
        assert_eq!(options.default_level.as_deref(), Some("info"));
        assert_eq!(options.dir, Some(PathBuf::from("logs")));
        assert!(!options.stdout);
    }
}
