use crate::error::AppResult;
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{Array, DocumentMut, Item, Table, Value};

/// Ensure direnv trusts all my-agents task worktrees.
///
/// direnv blocks copied `.envrc` files in each git worktree unless the path is
/// allowed. Trusting the managed projects directory avoids one manual
/// `direnv allow` per task worktree.
pub fn ensure_my_agents_projects_whitelisted(projects_dir: &Path) -> AppResult<()> {
    let config_path = direnv_config_path()?;
    ensure_direnv_whitelist_prefix(&config_path, projects_dir)?;
    Ok(())
}

fn direnv_config_path() -> AppResult<PathBuf> {
    if let Some(xdg_config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg_config_home.is_empty() {
            return Ok(PathBuf::from(xdg_config_home)
                .join("direnv")
                .join("direnv.toml"));
        }
    }

    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(home.join(".config").join("direnv").join("direnv.toml"))
}

fn ensure_direnv_whitelist_prefix(config_path: &Path, prefix_dir: &Path) -> AppResult<bool> {
    let prefix = whitelist_prefix(prefix_dir)?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = if config_path.exists() {
        fs::read_to_string(config_path)
            .with_context(|| format!("reading {}", config_path.display()))?
    } else {
        String::new()
    };

    let mut doc = if content.trim().is_empty() {
        DocumentMut::new()
    } else {
        content
            .parse::<DocumentMut>()
            .with_context(|| format!("parsing {}", config_path.display()))?
    };

    let whitelist = doc
        .as_table_mut()
        .entry("whitelist")
        .or_insert_with(|| Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[whitelist] in direnv.toml is not a table"))?;

    if !whitelist.contains_key("prefix") {
        let mut prefixes = Array::new();
        prefixes.push(prefix.as_str());
        whitelist.insert("prefix", Item::Value(Value::Array(prefixes)));
        fs::write(config_path, doc.to_string())?;
        return Ok(true);
    }

    let prefixes = whitelist
        .get_mut("prefix")
        .and_then(Item::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("[whitelist].prefix in direnv.toml is not an array"))?;

    if prefixes
        .iter()
        .any(|value| value.as_str() == Some(prefix.as_str()))
    {
        return Ok(false);
    }

    prefixes.push(prefix.as_str());
    fs::write(config_path, doc.to_string())?;
    Ok(true)
}

fn whitelist_prefix(prefix_dir: &Path) -> AppResult<String> {
    let absolute = if prefix_dir.is_absolute() {
        prefix_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(prefix_dir)
    };

    let mut prefix = absolute.to_string_lossy().to_string();
    if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
        prefix.push(std::path::MAIN_SEPARATOR);
    }
    Ok(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_direnv_config_with_projects_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("direnv").join("direnv.toml");
        let projects_dir = temp.path().join(".my-agents").join("projects");

        let changed = ensure_direnv_whitelist_prefix(&config_path, &projects_dir).unwrap();

        assert!(changed);
        let content = fs::read_to_string(config_path).unwrap();
        assert!(content.contains("[whitelist]"));
        assert!(content.contains(format!("\"{}/\"", projects_dir.to_string_lossy()).as_str()));
    }

    #[test]
    fn appends_projects_prefix_to_existing_whitelist() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("direnv.toml");
        fs::write(
            &config_path,
            "[whitelist]\nexact = [\"/tmp/project/.envrc\"]\nprefix = [\"/tmp/existing/\"]\n",
        )
        .unwrap();

        let projects_dir = temp.path().join(".my-agents").join("projects");
        let changed = ensure_direnv_whitelist_prefix(&config_path, &projects_dir).unwrap();

        assert!(changed);
        let content = fs::read_to_string(config_path).unwrap();
        assert!(content.contains("\"/tmp/existing/\""));
        assert!(content.contains(format!("\"{}/\"", projects_dir.to_string_lossy()).as_str()));
    }

    #[test]
    fn does_not_duplicate_existing_projects_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("direnv.toml");
        let projects_dir = temp.path().join(".my-agents").join("projects");
        let prefix = whitelist_prefix(&projects_dir).unwrap();
        fs::write(
            &config_path,
            format!("[whitelist]\nprefix = [\"{}\"]\n", prefix),
        )
        .unwrap();

        let changed = ensure_direnv_whitelist_prefix(&config_path, &projects_dir).unwrap();

        assert!(!changed);
        let content = fs::read_to_string(config_path).unwrap();
        assert_eq!(content.matches(prefix.as_str()).count(), 1);
    }
}
