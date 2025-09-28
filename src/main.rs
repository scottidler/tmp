mod config;

use clap::{Arg, Command};
use config::{Config, Kind, load_config};
use eyre::{Context, Result};
use log::{debug, error, info, warn};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Tmp {
    kinds: Vec<Kind>,
}

impl Tmp {
    fn new(config: Config) -> Self {
        debug!("Creating Tmp instance with {len} kinds", len = config.kinds.len());

        let kinds = config
            .kinds
            .into_iter()
            .map(|mut kind| {
                debug!("Processing kind: {name}", name = kind.name);

                // Interpolate templates
                for (template_name, template_content) in &config.templates {
                    let pattern = format!("{{{template_name}}}");
                    kind.content = kind.content.replace(&pattern, template_content);
                }

                kind
            })
            .collect();

        Self { kinds }
    }

    fn find_kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.name == name)
    }

    fn create_file(&self, kind_name: &str, filename: &str) -> Result<()> {
        info!("Creating file: {filename} with kind: {kind_name}");

        let kind = self
            .find_kind(kind_name)
            .ok_or_else(|| eyre::eyre!("Kind '{kind_name}' not found"))?;

        let full_filename = if kind.suffix.is_empty() {
            filename.to_string()
        } else {
            let suffix_with_dot = format!(".{suffix}", suffix = kind.suffix);
            if filename.ends_with(&suffix_with_dot) {
                filename.to_string()
            } else {
                format!("{filename}.{suffix}", suffix = kind.suffix)
            }
        };

        debug!("Full filename: {full_filename}");

        if Path::new(&full_filename).exists() {
            warn!("File {full_filename} already exists, skipping creation");
            return Ok(());
        }

        let mut file =
            File::create(&full_filename).with_context(|| format!("Failed to create file: {full_filename}"))?;

        file.write_all(kind.content.as_bytes())
            .with_context(|| format!("Failed to write content to file: {full_filename}"))?;

        if let Some(chmod) = kind.chmod {
            debug!("Setting permissions to {chmod:o} for file: {full_filename}");
            let permissions = std::fs::Permissions::from_mode(chmod);
            fs::set_permissions(&full_filename, permissions)
                .with_context(|| format!("Failed to set permissions for file: {full_filename}"))?;
        }

        info!("Successfully created file: {full_filename}");
        Ok(())
    }

    fn delete_file(&self, kind_name: &str, filename: &str) -> Result<()> {
        info!("Deleting file: {filename} with kind: {kind_name}");

        let kind = self
            .find_kind(kind_name)
            .ok_or_else(|| eyre::eyre!("Kind '{kind_name}' not found"))?;

        let full_filename = if kind.suffix.is_empty() {
            filename.to_string()
        } else {
            let suffix_with_dot = format!(".{suffix}", suffix = kind.suffix);
            if filename.ends_with(&suffix_with_dot) {
                filename.to_string()
            } else {
                format!("{filename}.{suffix}", suffix = kind.suffix)
            }
        };

        debug!("Full filename to delete: {full_filename}");

        if !Path::new(&full_filename).exists() {
            warn!("File {full_filename} does not exist, nothing to delete");
            return Ok(());
        }

        fs::remove_file(&full_filename).with_context(|| format!("Failed to delete file: {full_filename}"))?;

        info!("Successfully deleted file: {full_filename}");
        Ok(())
    }

    fn list_kinds(&self) {
        info!("Listing available kinds:");
        for kind in &self.kinds {
            println!("{}", kind.name);
        }
    }
}

fn setup_logging() -> Result<()> {
    // Create log directory if it doesn't exist
    let home = std::env::var("HOME").context("HOME environment variable not set")?;
    let log_dir = Path::new(&home).join(".local/share/tmp");

    if !log_dir.exists() {
        fs::create_dir_all(&log_dir).context("Failed to create log directory")?;
    }

    // Set up file-based logging
    let log_file = log_dir.join("tmp.log");

    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(Box::new(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
                .with_context(|| format!("Failed to open log file: {log_file:?}"))?,
        )))
        .init();

    info!("Logging initialized, writing to: {log_file:?}");
    Ok(())
}

fn main() -> Result<()> {
    setup_logging().context("Failed to setup logging")?;

    info!("Starting tmp application");

    let matches = Command::new("tmp")
        .version("0.1.0")
        .author("Scott Idler")
        .about("Helper function to quickly make file types defined in the config file")
        .arg(
            Arg::new("config")
                .long("config")
                .value_name("FILEPATH")
                .help("Config filepath")
                .default_value("~/.config/tmp/tmp.yml"),
        )
        .arg(
            Arg::new("nerf")
                .short('N')
                .long("nerf")
                .action(clap::ArgAction::SetTrue)
                .help("Only print contents of the file to be made"),
        )
        .arg(
            Arg::new("rm")
                .short('r')
                .long("rm")
                .action(clap::ArgAction::SetTrue)
                .help("Delete filename"),
        )
        .arg(
            Arg::new("chmod")
                .short('c')
                .long("chmod")
                .value_name("MODE")
                .help("Set the value to chmod the file to"),
        )
        .arg(
            Arg::new("kind")
                .value_name("KIND")
                .help("Choose which kind of tmp file")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("name")
                .value_name("NAME")
                .help("Optionally name the script")
                .index(2),
        )
        .get_matches();

    debug!("Parsed command line arguments");

    // Expand tilde in config path
    let config_path = matches.get_one::<String>("config").unwrap();
    let config_path = if config_path.starts_with('~') {
        let home = std::env::var("HOME").context("HOME environment variable not set")?;
        PathBuf::from(config_path.replacen('~', &home, 1))
    } else {
        PathBuf::from(config_path)
    };

    debug!("Resolved config path: {config_path:?}");

    let config = load_config(&config_path).with_context(|| format!("Failed to load config from {config_path:?}"))?;

    let app = Tmp::new(config);

    let kind = matches.get_one::<String>("kind").unwrap();
    let name = matches.get_one::<String>("name").map(|s| s.as_str());
    let nerf = matches.get_flag("nerf");
    let rm = matches.get_flag("rm");
    let chmod = matches
        .get_one::<String>("chmod")
        .map(|s| u32::from_str_radix(s, 8))
        .transpose()
        .context("Invalid chmod value, must be octal")?;

    debug!("Processing request - kind: {kind}, name: {name:?}, nerf: {nerf}, rm: {rm}, chmod: {chmod:?}");

    // Validate kind exists
    if app.find_kind(kind).is_none() {
        error!("Unknown kind: {kind}");
        eprintln!("Unknown kind: {kind}");
        app.list_kinds();
        return Err(eyre::eyre!("Unknown kind: {kind}"));
    }

    if nerf {
        info!("Nerf mode: printing file content");
        let kind_obj = app.find_kind(kind).unwrap();
        println!("{}", kind_obj.content);
    } else if rm {
        let kind_obj = app.find_kind(kind).unwrap();
        let default_filename = format!("tmp.{suffix}", suffix = kind_obj.suffix);
        let filename = name.unwrap_or(&default_filename);
        info!("Remove mode: deleting file: {filename}");
        app.delete_file(kind, filename)
            .with_context(|| format!("Failed to delete file: {filename}"))?;
    } else {
        info!("Create mode: creating file");
        let kind_obj = app.find_kind(kind).unwrap();
        let default_filename = format!("tmp.{suffix}", suffix = kind_obj.suffix);
        let filename = name.unwrap_or(&default_filename);
        app.create_file(kind, filename)
            .with_context(|| format!("Failed to create file of kind: {kind}"))?;
    }

    info!("tmp application completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn test_template_interpolation() {
        let kinds = vec![Kind {
            name: "test".to_string(),
            chmod: Some(0o755),
            suffix: "sh".to_string(),
            content: "{header}\necho {message}".to_string(),
        }];

        let mut templates = HashMap::new();
        templates.insert("header".to_string(), "#!/bin/bash".to_string());
        templates.insert("message".to_string(), "Hello World".to_string());

        let processed = Tmp::new(Config { kinds, templates });

        assert_eq!(processed.kinds.len(), 1);
        let kind = &processed.kinds[0];
        assert_eq!(kind.name, "test");
        assert_eq!(kind.content, "#!/bin/bash\necho Hello World");
    }

    #[test]
    fn test_find_kind() {
        let kinds = vec![
            Kind {
                name: "first".to_string(),
                chmod: Some(0o644),
                suffix: "txt".to_string(),
                content: "content1".to_string(),
            },
            Kind {
                name: "second".to_string(),
                chmod: Some(0o755),
                suffix: "sh".to_string(),
                content: "content2".to_string(),
            },
        ];

        let config = Config {
            kinds: kinds.clone(),
            templates: HashMap::new(),
        };

        let tmp = Tmp::new(config);

        let found = tmp.find_kind("second").unwrap();
        assert_eq!(found.name, "second");
        assert_eq!(found.chmod, Some(0o755));

        assert!(tmp.find_kind("nonexistent").is_none());
    }

    #[test]
    fn test_create_file() {
        let tempdir = tempdir().unwrap();
        let file_path = tempdir.path().join("test-file.sh");

        let kinds = vec![Kind {
            name: "test".to_string(),
            chmod: Some(0o755),
            suffix: "sh".to_string(),
            content: "#!/bin/bash\necho 'test'\n".to_string(),
        }];

        let config = Config {
            kinds,
            templates: HashMap::new(),
        };

        let tmp = Tmp::new(config);

        // Use absolute path for filename
        let filename_without_suffix = file_path.with_extension("").to_string_lossy().to_string();
        tmp.create_file("test", &filename_without_suffix).unwrap();

        // Verify file exists
        assert!(file_path.exists());

        // Verify content
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "#!/bin/bash\necho 'test'\n");

        // Verify permissions
        let metadata = fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o755);
    }

    #[test]
    fn test_delete_file() {
        let tempdir = tempdir().unwrap();
        let file_path = tempdir.path().join("to-delete.txt");

        // Create a file first
        fs::write(&file_path, "content").unwrap();
        assert!(file_path.exists());

        let kinds = vec![Kind {
            name: "test".to_string(),
            chmod: None,
            suffix: "txt".to_string(),
            content: "content".to_string(),
        }];

        let config = Config {
            kinds,
            templates: HashMap::new(),
        };

        let tmp = Tmp::new(config);

        // Use absolute path for filename without suffix
        let filename_without_suffix = file_path.with_extension("").to_string_lossy().to_string();
        tmp.delete_file("test", &filename_without_suffix).unwrap();

        assert!(!file_path.exists());
    }

    #[test]
    fn test_create_file_unknown_kind() {
        let config = Config {
            kinds: vec![],
            templates: HashMap::new(),
        };

        let tmp = Tmp::new(config);

        let result = tmp.create_file("unknown", "test.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Kind 'unknown' not found"));
    }

    #[test]
    fn test_chmod_default_value() {
        let tempdir = tempdir().unwrap();
        let file_path = tempdir.path().join("no-chmod.txt");

        let kinds = vec![Kind {
            name: "no-chmod".to_string(),
            chmod: None,
            suffix: "txt".to_string(),
            content: "content".to_string(),
        }];

        let config = Config {
            kinds,
            templates: HashMap::new(),
        };

        let tmp = Tmp::new(config);

        // Use absolute path for filename without suffix
        let filename_without_suffix = file_path.with_extension("").to_string_lossy().to_string();
        tmp.create_file("no-chmod", &filename_without_suffix).unwrap();

        let metadata = fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions();
        // Should default to whatever the system default is (0o664 = 436 decimal)
        assert_eq!(permissions.mode() & 0o777, 0o664);
    }

    #[test]
    fn test_integration_template_interpolation() {
        // Test with real config file if it exists
        let config_path = std::path::Path::new(&std::env::var("HOME").unwrap()).join(".config/tmp/tmp.yml");

        if config_path.exists() {
            let config = load_config(&config_path).unwrap();
            let tmp = Tmp::new(config);

            // Find the 'py' kind which should have template interpolation
            if let Some(py_kind) = tmp.find_kind("py") {
                // Verify that templates have been interpolated (no more {template} placeholders)
                assert!(!py_kind.content.contains("{py3-header}"));
                assert!(!py_kind.content.contains("{py-common}"));
                assert!(!py_kind.content.contains("{py-footer}"));

                // Verify that actual content has been interpolated
                assert!(py_kind.content.contains("#!/usr/bin/env python3"));
                assert!(py_kind.content.contains("if __name__ == '__main__':"));
                assert!(py_kind.content.contains("import os"));
            }
        }
    }

    #[test]
    fn test_integration_file_creation_with_templates() {
        use tempfile::tempdir;

        // Test with real config file if it exists
        let config_path = std::path::Path::new(&std::env::var("HOME").unwrap()).join(".config/tmp/tmp.yml");

        if config_path.exists() {
            let config = load_config(&config_path).unwrap();
            let tmp = Tmp::new(config);

            // Test creating a file in a temporary directory
            let tempdir = tempdir().unwrap();
            let test_file_path = tempdir.path().join("test-integration");

            // Create a py file
            if tmp.find_kind("py").is_some() {
                let filename = test_file_path.to_string_lossy();
                tmp.create_file("py", &filename).unwrap();

                let created_file = tempdir.path().join("test-integration.py");
                assert!(created_file.exists());

                // Read the content and verify template interpolation worked
                let content = fs::read_to_string(&created_file).unwrap();
                assert!(content.contains("#!/usr/bin/env python3"));
                assert!(content.contains("if __name__ == '__main__':"));
                assert!(content.contains("def main(args):"));
                assert!(!content.contains("{py3-header}"));
                assert!(!content.contains("{py-footer}"));
            }
        }
    }

    #[test]
    fn test_chmod_from_config() {
        let tempdir = tempdir().unwrap();
        let file_path = tempdir.path().join("chmod-test.sh");

        let kinds = vec![Kind {
            name: "test-exec".to_string(),
            chmod: Some(509), // This should be decimal 509 = octal 775
            suffix: "sh".to_string(),
            content: "#!/bin/bash\necho test".to_string(),
        }];

        let config = Config {
            kinds,
            templates: HashMap::new(),
        };

        let tmp = Tmp::new(config);

        // Use absolute path for filename without suffix
        let filename_without_suffix = file_path.with_extension("").to_string_lossy().to_string();
        tmp.create_file("test-exec", &filename_without_suffix).unwrap();

        let metadata = fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions();
        // Should be 0o775 (509 decimal) = rwxrwxr-x
        assert_eq!(permissions.mode() & 0o777, 0o775);
    }
}
