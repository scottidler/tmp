use eyre::{Context, Result};
use log::{debug, error, info};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Kind {
    pub name: String,
    pub chmod: Option<u32>,
    pub suffix: String,
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(deserialize_with = "deserialize_kinds")]
    pub kinds: Vec<Kind>,
    pub templates: HashMap<String, String>,
}

fn deserialize_kinds<'de, D>(deserializer: D) -> Result<Vec<Kind>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, KindData> = HashMap::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .map(|(name, mut data)| {
            // Handle chmod values that are likely meant to be octal
            // Common octal values like 755, 775, 644, etc. when written as decimal
            // should be interpreted as octal for backward compatibility
            if let Some(chmod) = data.chmod {
                data.chmod = Some(interpret_chmod_value(chmod));
            }

            Kind {
                name,
                chmod: data.chmod,
                suffix: data.suffix,
                content: data.content,
            }
        })
        .collect())
}

fn interpret_chmod_value(value: u32) -> u32 {
    // Check if the value looks like a common octal permission written as decimal
    // Common patterns: 644, 664, 755, 775, 777, etc.
    match value {
        644 => 0o644, // rw-r--r--
        664 => 0o664, // rw-rw-r--
        755 => 0o755, // rwxr-xr-x
        775 => 0o775, // rwxrwxr-x
        777 => 0o777, // rwxrwxrwx
        600 => 0o600, // rw-------
        700 => 0o700, // rwx------
        744 => 0o744, // rwxr--r--
        _ => {
            // If it's already a reasonable file permission value (< 0o777), use as-is
            // Otherwise, try to interpret as octal digits written as decimal
            if value <= 0o777 {
                value
            } else {
                // Try to parse as octal digits (e.g., 775 -> 0o775)
                let octal_str = value.to_string();
                if octal_str.chars().all(|c| c.is_ascii_digit() && c <= '7') {
                    u32::from_str_radix(&octal_str, 8).unwrap_or(value)
                } else {
                    value
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct KindData {
    chmod: Option<u32>,
    suffix: String,
    content: String,
}

pub fn load_config(path: &Path) -> Result<Config> {
    debug!("Loading config from: {path:?}");

    if !path.exists() {
        error!("Config file not found: {path:?}");
        return Err(eyre::eyre!("Config file not found: {path:?}"));
    }

    let content = fs::read_to_string(path).with_context(|| format!("Failed to read config file: {path:?}"))?;

    debug!("Config file content length: {len} bytes", len = content.len());

    let config: Config =
        serde_yaml::from_str(&content).with_context(|| format!("Failed to parse YAML config: {path:?}"))?;

    info!("Successfully loaded config from: {path:?}");
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_load_config_file_not_found() {
        let result = load_config(Path::new("/nonexistent/path/config.yml"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Config file not found"));
    }

    #[test]
    fn test_load_config_invalid_yaml() {
        let tempdir = tempdir().unwrap();
        let temp_file = tempdir.path().join("invalid.yml");
        fs::write(&temp_file, "invalid: yaml: content: [").unwrap();

        let result = load_config(&temp_file);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse YAML"));
    }

    #[test]
    fn test_load_config_valid() {
        let yaml_content = "kinds:\n  test:\n    chmod: 755\n    suffix: sh\n    content: |\n      echo test\ntemplates:\n  header: \"bash header\"";

        let tempdir = tempdir().unwrap();
        let temp_file = tempdir.path().join("valid.yml");
        fs::write(&temp_file, yaml_content).unwrap();

        let config = load_config(&temp_file).unwrap();

        assert_eq!(config.kinds.len(), 1);
        assert_eq!(config.templates.len(), 1);

        let kind = &config.kinds[0];
        assert_eq!(kind.name, "test");
        assert_eq!(kind.chmod, Some(0o755));
        assert_eq!(kind.suffix, "sh");
    }

    #[test]
    fn test_kind_deserialization() {
        let yaml = "kinds:\n  test-kind:\n    chmod: 755\n    suffix: sh\n    content: |\n      echo test\n  another-kind:\n    suffix: py\n    content: |\n      print hello\ntemplates:\n  header: \"Header\"";

        let config: Config = serde_yaml::from_str(yaml).expect("Failed to parse config");

        assert_eq!(config.kinds.len(), 2);
        assert_eq!(config.templates.len(), 1);

        let test_kind = config.kinds.iter().find(|k| k.name == "test-kind").unwrap();
        assert_eq!(test_kind.chmod, Some(0o755));
        assert_eq!(test_kind.suffix, "sh");
        assert!(test_kind.content.contains("echo test"));

        let another_kind = config.kinds.iter().find(|k| k.name == "another-kind").unwrap();
        assert_eq!(another_kind.chmod, None);
        assert_eq!(another_kind.suffix, "py");
        assert!(another_kind.content.contains("print hello"));
    }

    #[test]
    fn test_chmod_interpretation() {
        let yaml = "kinds:\n  test-script:\n    chmod: 775\n    suffix: sh\n    content: |\n      echo test\ntemplates:\n  header: \"Header\"";

        let config: Config = serde_yaml::from_str(yaml).expect("Failed to parse config");

        assert_eq!(config.kinds.len(), 1);

        let kind = &config.kinds[0];
        assert_eq!(kind.name, "test-script");
        // 775 in config should be interpreted as octal 775 = decimal 509
        assert_eq!(kind.chmod, Some(0o775));
        assert_eq!(kind.suffix, "sh");
    }

    #[test]
    fn test_chmod_interpretation_edge_cases() {
        // Test that already-correct decimal values are preserved
        assert_eq!(interpret_chmod_value(509), 509); // Already correct decimal for 0o775
        assert_eq!(interpret_chmod_value(420), 420); // Already correct decimal for 0o644

        // Test common octal-as-decimal interpretations
        assert_eq!(interpret_chmod_value(755), 0o755); // 755 -> 0o755 (493 decimal)
        assert_eq!(interpret_chmod_value(775), 0o775); // 775 -> 0o775 (509 decimal)
        assert_eq!(interpret_chmod_value(644), 0o644); // 644 -> 0o644 (420 decimal)

        // Test invalid octal digits (should remain unchanged)
        assert_eq!(interpret_chmod_value(789), 789); // Contains 8,9 - not valid octal
    }
}
