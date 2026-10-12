//! Permission requests from the agent and the user's answers.
use super::*;

impl Worker {
    pub(super) fn permission_request(
        &mut self,
        request: &wire::RequestPermissionRequest,
        responder: Responder<wire::RequestPermissionResponse>,
    ) {
        if Some(request.session_id.0.as_ref()) != self.session_id.as_deref()
            || self
                .active
                .as_ref()
                .is_none_or(|turn| turn.cancelled || turn.prompt_request.is_none())
        {
            respond(Some(responder), None);
            return;
        }
        let options: Vec<_> = request
            .options
            .iter()
            .filter_map(|option| {
                let kind = match option.kind {
                    wire::PermissionOptionKind::AllowOnce => PermissionKind::AllowOnce,
                    wire::PermissionOptionKind::AllowAlways => PermissionKind::AllowAlways,
                    wire::PermissionOptionKind::RejectOnce => PermissionKind::RejectOnce,
                    // "Reject always" stays hidden.
                    _ => return None,
                };
                Some(PermissionOption {
                    id: option.option_id.0.to_string(),
                    name: option.name.clone(),
                    kind,
                })
            })
            .collect();
        if options.is_empty() {
            respond(Some(responder), None);
            if let Some(turn) = &mut self.active {
                turn.denied = true;
            }
            return;
        }
        let tool = &request.tool_call.fields;
        let mut title = tool
            .title
            .clone()
            .unwrap_or_else(|| "Agent requests permission".to_owned());
        let detail = tool_detail(tool.raw_output.as_ref(), tool.content.as_deref());
        if !detail.is_empty() {
            title.push('\n');
            title.push_str(&detail);
        }
        if let Some(input) = tool.raw_input.as_ref().filter(|input| !input.is_null()) {
            use std::fmt::Write as _;
            let _ = write!(title, "\nInput: {input}");
        }
        self.next_permission += 1;
        let request_id = self.next_permission;
        self.permissions.insert(
            request_id,
            PendingPermission {
                turn: self.turn,
                options: options.clone(),
                responder: Some(responder),
            },
        );
        self.emit(EventKind::Permission {
            request_id,
            title,
            options,
        });
    }

    pub(super) fn permission(&mut self, id: u64, option_id: &str) {
        let Some(mut pending) = self.permissions.remove(&id) else {
            return;
        };
        if pending.turn != self.turn || self.active.as_ref().is_none_or(|active| active.cancelled) {
            respond(pending.responder.take(), None);
            return;
        }
        if let Some(option) = pending.options.iter().find(|option| option.id == option_id) {
            let denied = option.kind == PermissionKind::RejectOnce;
            respond(pending.responder.take(), Some(option_id));
            if denied && let Some(active) = &mut self.active {
                active.denied = true;
            }
        } else {
            respond(pending.responder.take(), None);
        }
    }

    pub(super) fn cancel_permissions(&mut self) {
        for (_, mut pending) in self.permissions.drain() {
            respond(pending.responder.take(), None);
        }
    }
}

pub(super) fn respond(
    responder: Option<Responder<wire::RequestPermissionResponse>>,
    option: Option<&str>,
) {
    let Some(responder) = responder else {
        return;
    };
    let outcome = option.map_or(wire::RequestPermissionOutcome::Cancelled, |option| {
        wire::RequestPermissionOutcome::Selected(wire::SelectedPermissionOutcome::new(
            option.to_owned(),
        ))
    });
    let _ = responder.respond(wire::RequestPermissionResponse::new(outcome));
}
