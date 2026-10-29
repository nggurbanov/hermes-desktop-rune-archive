use super::*;
use crate::pages::capabilities::LogSource;
pub(super) fn panel(s: &Viewer, k: &Kit) -> El {
    let logs = &s.capabilities.mcp_logs;
    let mut pane = col().w_full().gap(7.0).child(heading("MCP logs", k)).child(
        row()
            .gap(6.0)
            .child(button(
                "Server output",
                Action::McpLogs(LogSource::Stdio),
                false,
                k,
            ))
            .child(button(
                "Agent log",
                Action::McpLogs(LogSource::Agent),
                false,
                k,
            )),
    );
    if !logs.opened {
        return pane;
    }
    pane = pane.child(
        row()
            .gap(6.0)
            .child(button("Refresh logs", Action::RefreshMcpLogs, false, k))
            .child(button("Hide logs", Action::HideMcpLogs, false, k)),
    );
    if let Some(error) = &logs.error {
        pane = pane.child(notice(error, k).role("alert"));
    }
    if logs.lines.is_empty() && logs.error.is_none() {
        pane = pane.child(notice(
            if s.capabilities.mcp_logs_loading() {
                "Loading log lines…"
            } else {
                "No log lines for this server in the selected source."
            },
            k,
        ));
    }
    let mut output = col().w_full().gap(2.0);
    for line in &logs.lines {
        output = output.child(text(line).font(k.ty.small).color(k.c.text_base).wrap());
    }
    pane.child(
        output
            .max_h(300.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .key("mcp-log-lines"),
    )
}
