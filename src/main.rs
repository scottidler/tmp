use clap::{Arg, Command};
use eyre::{Context, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Kind {
    name: String,
    chmod: Option<u32>,
    suffix: String,
    content: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Config {
    #[serde(deserialize_with = "deserialize_kinds")]
    kinds: Vec<Kind>,
    templates: HashMap<String, String>,
}

fn deserialize_kinds<'de, D>(deserializer: D) -> Result<Vec<Kind>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, KindData> = HashMap::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .map(|(name, data)| Kind {
            name,
            chmod: data.chmod,
            suffix: data.suffix,
            content: data.content,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct KindData {
    chmod: Option<u32>,
    suffix: String,
    content: String,
}

#[derive(Debug)]
struct Tmp {
    kinds: Vec<Kind>,
}

fn load_config(path: &Path) -> Result<Config> {
    debug!("Loading config from: {:?}", path);
    
    if !path.exists() {
        error!("Config file not found: {:?}", path);
        return Err(eyre::eyre!("Config file not found: {:?}", path));
    }
    
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;
    
    debug!("Config file content length: {} bytes", content.len());
    
    let config: Config = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse YAML config: {:?}", path))?;
    
    info!("Successfully loaded config from: {:?}", path);
    Ok(config)
}

impl Tmp {
    fn new(config: Config) -> Result<Self> {
        info!("Initializing tmp application");
        
        info!("Loaded config with {} kinds and {} templates", 
              config.kinds.len(), config.templates.len());
        
        let kinds = Self::interpolate_kinds(&config.kinds, &config.templates)
            .context("Failed to interpolate kinds with templates")?;
        
        debug!("Processed {} kinds", kinds.len());
        
        Ok(Self {
            kinds,
        })
    }
    
    fn interpolate_kinds(
        kinds: &[Kind],
        templates: &HashMap<String, String>,
    ) -> Result<Vec<Kind>> {
        debug!("Starting interpolation of {} kinds", kinds.len());
        
        let mut processed = Vec::new();
        
        for kind in kinds {
            debug!("Processing kind: {}", kind.name);
            
            let mut content = kind.content.clone();
            
            // Replace template placeholders
            for (template_name, template_content) in templates {
                let placeholder = format!("{{{}}}", template_name);
                if content.contains(&placeholder) {
                    debug!("Replacing template '{}' in kind '{}'", template_name, kind.name);
                    content = content.replace(&placeholder, template_content);
                }
            }
            
            let processed_kind = Kind {
                name: kind.name.clone(),
                chmod: kind.chmod,
                suffix: kind.suffix.clone(),
                content,
            };
            
            debug!("Processed kind '{}' with chmod: {:?}, suffix: {}", 
                   processed_kind.name, processed_kind.chmod, processed_kind.suffix);
            
            processed.push(processed_kind);
        }
        
        info!("Successfully interpolated {} kinds", processed.len());
        Ok(processed)
    }
    
    fn find_kind(&self, name: &str) -> Option<&Kind> {
        self.kinds.iter().find(|k| k.name == name)
    }
    
    fn create_file(&self, kind_name: &str, filename: Option<&str>, custom_chmod: Option<u32>) -> Result<()> {
        let kind = self.find_kind(kind_name)
            .ok_or_else(|| eyre::eyre!("Unknown kind: {}", kind_name))?;
        
        let default_filename = format!("tmp.{}", kind.suffix);
        let filename = filename.unwrap_or(&default_filename);
        let filepath = Path::new(filename);
        
        info!("Creating file: {} of kind: {}", filename, kind_name);
        debug!("File content length: {} bytes", kind.content.len());
        
        // Create parent directories if they don't exist
        if let Some(parent) = filepath.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                debug!("Creating parent directories: {:?}", parent);
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create parent directories: {:?}", parent))?;
                info!("Created parent directories: {:?}", parent);
            }
        }
        
        // Write file content
        fs::write(filepath, &kind.content)
            .with_context(|| format!("Failed to write file: {}", filename))?;
        
        info!("Successfully wrote file: {}", filename);
        
        // Set file permissions
        let chmod = custom_chmod.unwrap_or(kind.chmod.unwrap_or(0o644));
        debug!("Setting file permissions to: {:o}", chmod);
        
        let metadata = fs::metadata(filepath)
            .with_context(|| format!("Failed to get metadata for: {}", filename))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(chmod);
        
        fs::set_permissions(filepath, permissions)
            .with_context(|| format!("Failed to set permissions for: {}", filename))?;
        
        info!("Successfully set permissions {:o} for: {}", chmod, filename);
        Ok(())
    }
    
    fn delete_file(&self, filename: &str) -> Result<()> {
        let filepath = Path::new(filename);
        
        if !filepath.exists() {
            warn!("File does not exist, cannot delete: {}", filename);
            return Ok(());
        }
        
        info!("Deleting file: {}", filename);
        
        fs::remove_file(filepath)
            .with_context(|| format!("Failed to delete file: {}", filename))?;
        
        info!("Successfully deleted file: {}", filename);
        Ok(())
    }
    
    fn print_kind_info(&self, kind_name: &str) -> Result<()> {
        let kind = self.find_kind(kind_name)
            .ok_or_else(|| eyre::eyre!("Unknown kind: {}", kind_name))?;
        
        info!("Printing info for kind: {}", kind_name);
        
        println!("Kind: {}", kind.name);
        println!("Chmod: {:o}", kind.chmod.unwrap_or(0o644));
        println!("Suffix: {}", kind.suffix);
        println!("Content:");
        println!("{}", kind.content);
        
        Ok(())
    }
    
    fn get_available_kinds(&self) -> Vec<&String> {
        let mut kinds: Vec<&String> = self.kinds.iter().map(|k| &k.name).collect();
        kinds.sort();
        kinds
    }
}

fn setup_logging() -> Result<()> {
    // Create log directory if it doesn't exist
    let home = std::env::var("HOME")
        .context("HOME environment variable not set")?;
    let log_dir = Path::new(&home).join(".local/share/tmp");
    
    if !log_dir.exists() {
        fs::create_dir_all(&log_dir)
            .context("Failed to create log directory")?;
    }
    
    // Set up file-based logging
    let log_file = log_dir.join("tmp.log");
    
    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(Box::new(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
                .with_context(|| format!("Failed to open log file: {:?}", log_file))?
        )))
        .init();
    
    info!("Logging initialized, writing to: {:?}", log_file);
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
                .default_value("~/.config/tmp/tmp.yml")
        )
        .arg(
            Arg::new("nerf")
                .short('N')
                .long("nerf")
                .action(clap::ArgAction::SetTrue)
                .help("Only print contents of the file to be made")
        )
        .arg(
            Arg::new("rm")
                .short('r')
                .long("rm")
                .action(clap::ArgAction::SetTrue)
                .help("Delete filename")
        )
        .arg(
            Arg::new("chmod")
                .short('c')
                .long("chmod")
                .value_name("MODE")
                .help("Set the value to chmod the file to")
        )
        .arg(
            Arg::new("kind")
                .value_name("KIND")
                .help("Choose which kind of tmp file")
                .required(true)
                .index(1)
        )
        .arg(
            Arg::new("name")
                .value_name("NAME")
                .help("Optionally name the script")
                .index(2)
        )
        .get_matches();
    
    debug!("Parsed command line arguments");
    
    // Expand tilde in config path
    let config_path = matches.get_one::<String>("config").unwrap();
    let config_path = if config_path.starts_with('~') {
        let home = std::env::var("HOME")
            .context("HOME environment variable not set")?;
        PathBuf::from(config_path.replacen('~', &home, 1))
    } else {
        PathBuf::from(config_path)
    };
    
    debug!("Resolved config path: {:?}", config_path);
    
    let config = load_config(&config_path)
        .with_context(|| format!("Failed to load config from {:?}", config_path))?;
    
    let app = Tmp::new(config)
        .context("Failed to initialize tmp application")?;
    
    let kind = matches.get_one::<String>("kind").unwrap();
    let name = matches.get_one::<String>("name").map(|s| s.as_str());
    let nerf = matches.get_flag("nerf");
    let rm = matches.get_flag("rm");
    let chmod = matches.get_one::<String>("chmod")
        .map(|s| u32::from_str_radix(s, 8))
        .transpose()
        .context("Invalid chmod value, must be octal")?;
    
    debug!("Processing request - kind: {}, name: {:?}, nerf: {}, rm: {}, chmod: {:?}", 
           kind, name, nerf, rm, chmod);
    
    // Validate kind exists
    if app.find_kind(kind).is_none() {
        error!("Unknown kind: {}", kind);
        eprintln!("Unknown kind: {}", kind);
        let kinds: Vec<String> = app.get_available_kinds().iter().map(|s| s.to_string()).collect();
        eprintln!("Available kinds: {}", kinds.join(", "));
        return Err(eyre::eyre!("Unknown kind: {}", kind));
    }
    
    if nerf {
        info!("Nerf mode: printing kind info");
        app.print_kind_info(kind)
            .with_context(|| format!("Failed to print kind info for: {}", kind))?;
    } else if rm {
        let kind_obj = app.find_kind(kind).unwrap();
        let default_filename = format!("tmp.{}", kind_obj.suffix);
        let filename = name.unwrap_or(&default_filename);
        info!("Remove mode: deleting file: {}", filename);
        app.delete_file(filename)
            .with_context(|| format!("Failed to delete file: {}", filename))?;
    } else {
        info!("Create mode: creating file");
        app.create_file(kind, name, chmod)
            .with_context(|| format!("Failed to create file of kind: {}", kind))?;
    }
    
    info!("tmp application completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn test_template_interpolation() {
        let kinds = vec![
            Kind {
                name: "test".to_string(),
                chmod: Some(0o755),
                suffix: "sh".to_string(),
                content: "{header}\necho {message}".to_string(),
            }
        ];
        
        let mut templates = HashMap::new();
        templates.insert("header".to_string(), "#!/bin/bash".to_string());
        templates.insert("message".to_string(), "Hello World".to_string());
        
        let processed = Tmp::interpolate_kinds(&kinds, &templates).unwrap();
        
        assert_eq!(processed.len(), 1);
        let kind = &processed[0];
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
            }
        ];
        
        let config = Config {
            kinds: kinds.clone(),
            templates: HashMap::new(),
        };
        
        let tmp = Tmp::new(config).unwrap();
        
        let found = tmp.find_kind("second").unwrap();
        assert_eq!(found.name, "second");
        assert_eq!(found.chmod, Some(0o755));
        
        assert!(tmp.find_kind("nonexistent").is_none());
    }

    #[test]
    fn test_create_file() {
        let tempdir = tempdir().unwrap();
        let file_path = tempdir.path().join("test-file.sh");
        
        let kinds = vec![
            Kind {
                name: "test".to_string(),
                chmod: Some(0o755),
                suffix: "sh".to_string(),
                content: "#!/bin/bash\necho 'test'\n".to_string(),
            }
        ];
        
        let config = Config {
            kinds,
            templates: HashMap::new(),
        };
        
        let tmp = Tmp::new(config).unwrap();
        
        tmp.create_file("test", Some(file_path.to_str().unwrap()), None).unwrap();
        
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
        
        let config = Config {
            kinds: vec![],
            templates: HashMap::new(),
        };
        
        let tmp = Tmp::new(config).unwrap();
        
        tmp.delete_file(file_path.to_str().unwrap()).unwrap();
        
        assert!(!file_path.exists());
    }

    #[test]
    fn test_create_file_unknown_kind() {
        let config = Config {
            kinds: vec![],
            templates: HashMap::new(),
        };
        
        let tmp = Tmp::new(config).unwrap();
        
        let result = tmp.create_file("unknown", Some("test.txt"), None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unknown kind"));
    }

    #[test]
    fn test_chmod_default_value() {
        let kinds = vec![
            Kind {
                name: "no-chmod".to_string(),
                chmod: None,
                suffix: "txt".to_string(),
                content: "content".to_string(),
            }
        ];
        
        let tempdir = tempdir().unwrap();
        let file_path = tempdir.path().join("no-chmod.txt");
        
        let config = Config {
            kinds,
            templates: HashMap::new(),
        };
        
        let tmp = Tmp::new(config).unwrap();
        
        tmp.create_file("no-chmod", Some(file_path.to_str().unwrap()), None).unwrap();
        
        let metadata = fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions();
        // Should default to 0o644
        assert_eq!(permissions.mode() & 0o777, 0o644);
    }

    #[test]
    fn test_load_config_file_not_found() {
        let result = load_config(Path::new("/nonexistent/path/config.yml"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Config file not found"));
    }
}
