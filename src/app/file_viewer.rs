use std::path::{Path, PathBuf};

use crate::api::schema::PaneViewFileParams;
use crate::file_reference::FileReference;
use crate::layout::PaneId;

use super::App;

impl App {
    pub(crate) fn open_file_reference_in_viewer(
        &mut self,
        ws_idx: usize,
        source_pane_id: PaneId,
        reference: FileReference,
    ) -> Result<(), String> {
        let path = self.resolve_file_path_for_pane(ws_idx, source_pane_id, &reference.path)?;
        let target_pane_id = self
            .public_pane_id(ws_idx, source_pane_id)
            .ok_or_else(|| "source pane no longer exists".to_string())?;
        let response = self.runtime_pane_view_file(
            "ui.pane.view_file",
            PaneViewFileParams {
                target_pane_id,
                path: path.display().to_string(),
                line: reference.line,
                column: reference.column,
            },
        );

        if let Ok(error) = serde_json::from_str::<crate::api::schema::ErrorResponse>(&response) {
            return Err(error.error.message);
        }
        Ok(())
    }

    pub(super) fn resolve_file_path_for_pane(
        &self,
        ws_idx: usize,
        pane_id: PaneId,
        requested_path: &str,
    ) -> Result<PathBuf, String> {
        let requested_path = requested_path.trim();
        if requested_path.is_empty() || requested_path.contains('\0') {
            return Err("file path is empty or invalid".to_string());
        }

        let workspace = self
            .state
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| "workspace no longer exists".to_string())?;
        let tab_idx = workspace
            .find_tab_index_for_pane(pane_id)
            .ok_or_else(|| "source pane no longer exists".to_string())?;
        let source_cwd = workspace.tabs[tab_idx]
            .foreground_cwd_for_pane(pane_id, &self.terminal_runtimes)
            .or_else(|| {
                workspace.tabs[tab_idx].cwd_for_pane(
                    pane_id,
                    &self.state.terminals,
                    &self.terminal_runtimes,
                )
            })
            .unwrap_or_else(|| workspace.identity_cwd.clone());

        resolve_file_path(
            requested_path,
            &source_cwd,
            workspace
                .worktree_space()
                .map(|membership| membership.checkout_path.as_path())
                .unwrap_or(workspace.identity_cwd.as_path()),
        )
    }
}

pub(super) fn vim_read_only_argv(
    path: &Path,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<Vec<String>, String> {
    if line == Some(0) || column == Some(0) {
        return Err("line and column must be greater than zero".to_string());
    }
    if column.is_some() && line.is_none() {
        return Err("column requires a line number".to_string());
    }

    let mut argv = vec![
        "vim".to_string(),
        "-R".to_string(),
        "-M".to_string(),
        "-n".to_string(),
    ];
    if let Some(line) = line {
        let column = column.unwrap_or(1);
        argv.push(format!("+call cursor({line},{column})"));
    }
    argv.push("--".to_string());
    argv.push(path.display().to_string());
    Ok(argv)
}

fn resolve_file_path(
    requested_path: &str,
    source_cwd: &Path,
    workspace_root: &Path,
) -> Result<PathBuf, String> {
    let requested = expand_home(requested_path);
    let mut candidates = Vec::new();
    if requested.is_absolute() {
        candidates.push(requested);
    } else {
        candidates.push(source_cwd.join(&requested));
        candidates.push(workspace_root.join(&requested));
        if let Some(repo_root) = crate::workspace::git_repo_root(source_cwd) {
            candidates.push(repo_root.join(&requested));
        }
    }

    for candidate in candidates {
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if canonical.is_file() {
            return Ok(canonical);
        }
    }

    Err(format!("file not found: {requested_path}"))
}

fn expand_home(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix("~/") else {
        return PathBuf::from(path);
    };
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(rest))
        .unwrap_or_else(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::{resolve_file_path, vim_read_only_argv};
    use std::path::Path;

    #[test]
    fn vim_argv_enforces_read_only_mode_and_literal_path() {
        assert_eq!(
            vim_read_only_argv(Path::new("/tmp/a file.rs"), Some(44), Some(5)).unwrap(),
            vec![
                "vim",
                "-R",
                "-M",
                "-n",
                "+call cursor(44,5)",
                "--",
                "/tmp/a file.rs"
            ]
        );
    }

    #[test]
    fn vim_argv_rejects_invalid_locations() {
        assert!(vim_read_only_argv(Path::new("/tmp/a.rs"), Some(0), None).is_err());
        assert!(vim_read_only_argv(Path::new("/tmp/a.rs"), None, Some(2)).is_err());
    }

    #[test]
    fn resolver_checks_source_directory_then_workspace_root() {
        let test_root =
            std::env::temp_dir().join(format!("herdr-file-viewer-{}", std::process::id()));
        let source = test_root.join("nested");
        std::fs::create_dir_all(&source).unwrap();
        let file = test_root.join("README.md");
        std::fs::write(&file, "test").unwrap();

        assert_eq!(
            resolve_file_path("README.md", &source, &test_root).unwrap(),
            file.canonicalize().unwrap()
        );

        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_dir_all(test_root);
    }
}
