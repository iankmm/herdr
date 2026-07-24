use crate::api::schema::{EventData, EventEnvelope, EventKind, PaneViewFileParams, ResponseResult};
use crate::app::App;

use super::responses::{encode_error, encode_success};

impl App {
    pub(super) fn handle_pane_view_file(
        &mut self,
        id: String,
        params: PaneViewFileParams,
    ) -> String {
        let Some((ws_idx, source_pane_id)) = self.parse_pane_id(&params.target_pane_id) else {
            return encode_error(
                id,
                "pane_not_found",
                format!("pane {} not found", params.target_pane_id),
            );
        };
        let path = match self.resolve_file_path_for_pane(ws_idx, source_pane_id, &params.path) {
            Ok(path) => path,
            Err(message) => return encode_error(id, "file_not_found", message),
        };
        let argv = match super::super::file_viewer::vim_read_only_argv(
            &path,
            params.line,
            params.column,
        ) {
            Ok(argv) => argv,
            Err(message) => return encode_error(id, "invalid_file_location", message),
        };

        let existing_viewer =
            self.state
                .file_viewer_for_source(source_pane_id)
                .filter(|viewer_pane_id| {
                    self.find_pane(*viewer_pane_id)
                        .is_some_and(|(viewer_ws_idx, _)| viewer_ws_idx == ws_idx)
                });
        let split_target = existing_viewer.unwrap_or(source_pane_id);
        let previous_focus = self.state.current_pane_focus_target();
        let (rows, cols) = self.state.estimate_pane_size();
        let scrollback_limit_bytes = self.state.pane_scrollback_limit_bytes;
        let host_terminal_theme = self.state.host_terminal_theme;
        let viewer_cwd = path.parent().map(std::path::Path::to_path_buf);
        let split_result = self.state.workspaces[ws_idx].split_pane_viewer_argv_command_with_ratio(
            split_target,
            ratatui::layout::Direction::Horizontal,
            0.5,
            rows,
            cols,
            viewer_cwd,
            &argv,
            scrollback_limit_bytes,
            host_terminal_theme,
        );
        let (tab_idx, mut new_pane) = match split_result {
            Some(Ok(result)) => result,
            Some(Err(err)) => {
                return encode_error(id, "file_viewer_launch_failed", err.to_string())
            }
            None => {
                return encode_error(
                    id,
                    "pane_not_found",
                    format!("pane {} not found", params.target_pane_id),
                )
            }
        };
        new_pane.terminal.set_manual_label(format!(
            "view: {}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_else(|| path.as_os_str().to_str().unwrap_or("file"))
        ));

        self.state.switch_workspace_tab(ws_idx, tab_idx);
        self.state
            .record_pane_focus_change(previous_focus, ws_idx, new_pane.pane_id);
        self.state.settle_terminal_mode_after_focus();
        self.terminal_runtimes
            .insert(new_pane.terminal.id.clone(), new_pane.runtime);
        self.state
            .remove_alias_shadowed_by_new_pane(new_pane.pane_id);
        self.state
            .terminals
            .insert(new_pane.terminal.id.clone(), new_pane.terminal);

        let new_pane_id = new_pane.pane_id;
        let pane = self
            .pane_info(ws_idx, new_pane_id)
            .expect("newly created file viewer pane must have API metadata");
        self.emit_event(EventEnvelope {
            event: EventKind::PaneCreated,
            data: EventData::PaneCreated { pane: pane.clone() },
        });

        if let Some(old_viewer_pane_id) = existing_viewer {
            let old_public_pane_id = self.public_pane_id(ws_idx, old_viewer_pane_id);
            let old_terminal_id = self.state.terminal_id_for_pane(ws_idx, old_viewer_pane_id);
            let workspace_id = self.public_workspace_id(ws_idx);
            self.state.workspaces[ws_idx].close_pane(old_viewer_pane_id);
            self.state.remove_pane_local_records([old_viewer_pane_id]);
            self.state.remove_unattached_terminal_ids(old_terminal_id);
            self.shutdown_detached_terminal_runtimes();
            if let Some(old_public_pane_id) = old_public_pane_id {
                self.emit_event(EventEnvelope {
                    event: EventKind::PaneClosed,
                    data: EventData::PaneClosed {
                        pane_id: old_public_pane_id,
                        workspace_id,
                    },
                });
            }
        }

        self.state
            .set_file_viewer_for_source(source_pane_id, new_pane_id);
        self.schedule_session_save();
        self.emit_layout_updated_event(ws_idx, tab_idx);

        encode_success(id, ResponseResult::PaneInfo { pane })
    }
}
