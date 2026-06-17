use axum::http::HeaderMap;

use crate::{contracts::ActionActor, utils::empty_to_none};

pub(crate) fn actor_with_defaults(actor: Option<&ActionActor>) -> ActionActor {
    let actor = actor.cloned().unwrap_or_default();
    ActionActor {
        user_id: empty_to_none(&actor.user_id).unwrap_or_else(|| "velion-v3-local-user".into()),
        user_email: actor.user_email,
        user_name: actor.user_name,
        user_role: actor.user_role,
    }
}

pub(crate) fn actor_from_request(
    actor: Option<&ActionActor>,
    headers: Option<&HeaderMap>,
    allow_dev_actor_headers: bool,
) -> ActionActor {
    let header_actor = headers.map(|headers| actor_from_headers(headers, allow_dev_actor_headers));
    actor_with_defaults(if allow_dev_actor_headers {
        actor.or(header_actor.as_ref())
    } else {
        header_actor.as_ref()
    })
}

pub(crate) fn actor_from_headers(
    headers: &HeaderMap,
    allow_dev_actor_headers: bool,
) -> ActionActor {
    if let Some(actor) = trusted_actor_from_headers(headers) {
        return actor;
    }

    if allow_dev_actor_headers {
        return ActionActor {
            user_id: header_value(headers, "x-user-id").unwrap_or_default(),
            user_email: header_value(headers, "x-user-email").unwrap_or_default(),
            user_name: header_value(headers, "x-user-name").unwrap_or_default(),
            user_role: header_value(headers, "x-user-role").unwrap_or_default(),
        };
    }

    ActionActor::default()
}

fn trusted_actor_from_headers(headers: &HeaderMap) -> Option<ActionActor> {
    let user_id = header_value(headers, "x-session-user-id")
        .or_else(|| header_value(headers, "x-auth-user-id"))?;
    Some(ActionActor {
        user_id,
        user_email: header_value(headers, "x-session-user-email")
            .or_else(|| header_value(headers, "x-auth-user-email"))
            .unwrap_or_default(),
        user_name: header_value(headers, "x-session-user-name")
            .or_else(|| header_value(headers, "x-auth-user-name"))
            .unwrap_or_default(),
        user_role: header_value(headers, "x-session-user-role")
            .or_else(|| header_value(headers, "x-auth-role"))
            .unwrap_or_default(),
    })
}

fn header_value(headers: &HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
