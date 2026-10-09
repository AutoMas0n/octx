//! Permission-mode policy for ACP tool-call permission requests.

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, ToolKind,
};

use crate::cli::PermissionMode;

/// Whether a permission request is read-only for the purposes of `approve-reads`.
fn is_read_only(kind: Option<ToolKind>) -> bool {
    matches!(
        kind,
        Some(ToolKind::Read)
            | Some(ToolKind::Search)
            | Some(ToolKind::Fetch)
            | Some(ToolKind::Think)
    )
}

/// Select the first option matching one of `kinds`.
fn select(options: &[PermissionOption], kinds: &[PermissionOptionKind]) -> Option<String> {
    options
        .iter()
        .find(|o| kinds.contains(&o.kind))
        .map(|o| o.option_id.0.to_string())
}

fn selected(option_id: Option<String>) -> RequestPermissionResponse {
    match option_id {
        Some(id) => RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new(id),
        )),
        None => RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
    }
}

/// Decide a permission request according to the configured mode.
///
/// A denied or unanswerable request is reported back to the agent as
/// `Cancelled` (ACP's "not selected" outcome) rather than dropped.
#[must_use]
pub fn decide(
    mode: PermissionMode,
    request: &RequestPermissionRequest,
) -> RequestPermissionResponse {
    let allow = [
        PermissionOptionKind::AllowOnce,
        PermissionOptionKind::AllowAlways,
    ];
    let reject = [
        PermissionOptionKind::RejectOnce,
        PermissionOptionKind::RejectAlways,
    ];
    let kind = request.tool_call.fields.kind;

    match mode {
        PermissionMode::ApproveAll => selected(select(&request.options, &allow)),
        PermissionMode::DenyAll => selected(select(&request.options, &reject)),
        PermissionMode::ApproveReads => {
            if is_read_only(kind) {
                selected(select(&request.options, &allow))
            } else {
                selected(select(&request.options, &reject))
            }
        }
    }
}
