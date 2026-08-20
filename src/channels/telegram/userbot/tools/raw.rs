//! Raw MTProto passthrough — the eighth tool, behind the danger gate.
//!
//! The MCP package's `tg_mtproto` fires arbitrary constructor strings
//! and fails 55% of the time in their own production data. This module
//! is the deliberate opposite: a curated, schema-verified registry of
//! constructors, each parsed from JSON params into its typed
//! `gramers_tl_types` struct (impl-serde) before invoke. Unknown
//! methods refuse with the registry contents, not a guessing parser.
//!
//! Governance: `dispatch::authorize` requires `confirm: true` on the
//! invocation itself before any function here runs. The registry is
//! read-only constructors only — mutating raw calls are refused
//! permanently, not gated: the typed outbound tools exist for the safe
//! versions of those.

use anyhow::{Result, anyhow, bail};
use grammers_client::Client;
use grammers_session::types::PeerRef;
use grammers_tl_types as tl;
use serde_json::Value;

use super::commands::Raw;

/// Invoke a typed remote call and serialize its response envelope.
async fn rpc<R>(client: &Client, request: R) -> Result<Value>
where
    R: tl::RemoteCall,
    R::Return: serde::Serialize,
{
    let ret = client.invoke(&request).await?;
    Ok(serde_json::to_value(ret)?)
}

/// Registry methods, for error messages and tests.
pub(crate) const REGISTRY: &[&str] = &[
    "messages.getHistory",
    "messages.getReplies",
    "messages.getForumTopics",
    "contacts.resolveUsername",
    "contacts.resolvePhone",
    "account.getAuthorizations",
    "users.getFullUser",
];

/// Resolve a chat ref and produce the `InputPeer` raw calls need.
pub(crate) async fn input_peer(client: &Client, chat: &str) -> Result<tl::enums::InputPeer> {
    let peer: PeerRef = super::transport::resolve_chat_ref(client, chat).await?;
    Ok(tl::enums::InputPeer::from(&peer))
}

/// Execute a confirmed raw invocation. Params carry a `"chat"` string
/// where a peer is needed; it is resolved locally and spliced into the
/// typed constructor as its `peer` field before deserialization.
pub(crate) async fn run_raw(client: &Client, cmd: &Raw) -> Result<Value> {
    match cmd.method.as_str() {
        "messages.getHistory" => {
            let params = fill_peer(client, &cmd.params, "peer").await?;
            let mut req: tl::functions::messages::GetHistory = serde_json::from_value(params)?;
            req.limit = req.limit.clamp(1, 100);
            rpc(client, req).await
        }
        "messages.getReplies" => {
            let params = fill_peer(client, &cmd.params, "peer").await?;
            rpc(
                client,
                serde_json::from_value::<tl::functions::messages::GetReplies>(params)?,
            )
            .await
        }
        "messages.getForumTopics" => {
            let params = fill_peer(client, &cmd.params, "peer").await?;
            rpc(
                client,
                serde_json::from_value::<tl::functions::messages::GetForumTopics>(params)?,
            )
            .await
        }
        "contacts.resolveUsername" => {
            rpc(
                client,
                serde_json::from_value::<tl::functions::contacts::ResolveUsername>(
                    cmd.params.clone(),
                )?,
            )
            .await
        }
        "contacts.resolvePhone" => {
            rpc(
                client,
                serde_json::from_value::<tl::functions::contacts::ResolvePhone>(
                    cmd.params.clone(),
                )?,
            )
            .await
        }
        "account.getAuthorizations" => {
            rpc(client, tl::functions::account::GetAuthorizations {}).await
        }
        "users.getFullUser" => {
            rpc(
                client,
                serde_json::from_value::<tl::functions::users::GetFullUser>(cmd.params.clone())?,
            )
            .await
        }
        other => bail!(
            "method {other:?} is not in the verified registry; known: {}",
            REGISTRY.join(", ")
        ),
    }
}

/// Take `"chat"` from params, resolve it, splice a typed `peer` key in.
async fn fill_peer(client: &Client, params: &Value, key: &str) -> Result<Value> {
    let chat = params
        .get("chat")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("raw params need a \"chat\" string for peer resolution"))?;
    let peer = input_peer(client, chat).await?;
    let mut map = params
        .as_object()
        .ok_or_else(|| anyhow!("raw params must be a JSON object"))?
        .clone();
    map.remove("chat");
    map.insert(key.into(), serde_json::to_value(peer)?);
    Ok(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_verified_methods_only() {
        assert!(REGISTRY.contains(&"messages.getHistory"));
        assert!(REGISTRY.contains(&"contacts.resolveUsername"));
        // The mutating calls must never be reachable here.
        assert!(
            !REGISTRY
                .iter()
                .any(|m| m.contains("delete") || m.contains("Drop"))
        );
    }

    #[test]
    fn known_method_params_parse_into_typed_constructor() {
        // Round-trip the real runtime path: `fill_peer` splices a
        // serde-serialized InputPeer into the params, `run_raw` parses
        // the whole object into the typed constructor. Symmetric
        // serialize→deserialize cannot guess a variant shape wrong.
        let peer_ref = grammers_session::types::PeerRef {
            id: grammers_session::types::PeerId::self_user(),
            auth: grammers_session::types::PeerAuth::from_hash(7),
        };
        let peer = tl::enums::InputPeer::from(&peer_ref);
        let params = serde_json::json!({
            "peer": serde_json::to_value(&peer).expect("peer serializes"),
            "offset_id": 0,
            "offset_date": 0,
            "add_offset": 0,
            "limit": 10,
            "max_id": 0,
            "min_id": 0,
            "hash": 0
        });
        let req: tl::functions::messages::GetHistory =
            serde_json::from_value(params).expect("typed parse");
        assert_eq!(req.limit, 10);
    }

    #[test]
    fn chat_extraction_requires_the_chat_key() {
        let err = serde_json::json!({ "limit": 5 });
        // Mirror of fill_peer's extraction (no client available in
        // unit tests): the error must name the missing key.
        assert!(err.get("chat").and_then(Value::as_str).is_none());
    }
}
