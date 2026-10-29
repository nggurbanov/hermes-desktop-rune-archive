use super::*;
use crate::gateway::tests::{answer, http_answer, http_request, request};
use crate::gateway::{Connection, History, Message};
use std::{io::Write, net::TcpListener, thread, time::Duration};

fn payload() -> Value {
    json!({"provider":"custom:lab","model":"unlisted-current-model","providers":[
        {"slug":"lab","name":"Lab models","aliases":["custom:lab"],"authenticated":true,
         "api_url":"https://private.invalid/?token=not-a-real-secret","key_env":"PRIVATE_MODEL_KEY",
         "models":["agent-model","agent-model","plain-model","offline-model"],"unavailable_models":["offline-model"],
         "capabilities":{"agent-model":{"reasoning":true,"can_disable_reasoning":false,"fast":true,"ultrafast":true},
                         "plain-model":{"reasoning":false,"fast":false}}},
        {"slug":"unknown","name":"Unknown support","models":["future-model"]}
    ]})
}

#[test]
fn catalog_preserves_real_provider_model_identity_and_only_advertised_capabilities() {
    let catalog = ModelCatalog::from_value(&payload()).unwrap();
    assert_eq!(
        catalog.current_model.as_deref(),
        Some("unlisted-current-model")
    );
    let provider = &catalog.providers[0];
    assert!(provider.matches("custom:lab"));
    assert!(provider.matches("Lab models"));
    assert_eq!(provider.models.len(), 3);
    assert_eq!(provider.models[0].id, "agent-model");
    assert_eq!(provider.models[0].label, "agent-model");
    let caps = provider.models[0].capabilities.as_ref().unwrap();
    assert_eq!(caps.reasoning, Some(true));
    assert_eq!(caps.can_disable_reasoning, Some(false));
    assert_eq!(caps.fast, Some(true));
    assert_eq!(caps.ultrafast, Some(true));
    let plain = provider.models[1].capabilities.as_ref().unwrap();
    assert_eq!(plain.reasoning, Some(false));
    assert_eq!(plain.ultrafast, None);
    assert!(!provider.models[2].available);
    assert!(catalog.providers[1].models[0].capabilities.is_none());
    assert!(!format!("{catalog:?}").contains("not-a-real-secret"));
    assert!(!format!("{catalog:?}").contains("PRIVATE_MODEL_KEY"));
}

#[test]
fn catalog_refuses_oversized_or_ambiguous_collections_without_echoing_payloads() {
    let excessive = json!({"providers":vec![json!({"slug":"p","models":[]}); MAX_PROVIDERS+1]});
    assert!(
        ModelCatalog::from_value(&excessive)
            .unwrap_err()
            .contains("provider limit")
    );
    let excessive =
        json!({"providers":[{"slug":"p","models":vec!["x"; MAX_MODELS_PER_PROVIDER+1]}]});
    assert!(ModelCatalog::from_value(&excessive).is_err());
    let excessive = json!({"providers":(0..5).map(|i| json!({"slug":format!("p{i}"),"models":vec!["x"; MAX_MODELS_PER_PROVIDER]})).collect::<Vec<_>>()});
    assert!(
        ModelCatalog::from_value(&excessive)
            .unwrap_err()
            .contains("model limit")
    );
    assert!(ModelCatalog::from_value(&json!({"providers":[{"slug":"p"},{"slug":"p"}]})).is_err());
    let private_id = "private-value".repeat(MAX_ID_BYTES);
    let error =
        ModelCatalog::from_value(&json!({"providers":[{"slug":"p","models":[private_id]}]}))
            .unwrap_err();
    assert!(!error.contains("private-value"));
    assert!(ModelCatalog::from_value(&json!({"providers":[{"slug":"p","models":["m"],"capabilities":{"m":{"reasoning":"yes"}}}]})).is_err());
}

#[test]
fn catalog_rpc_uses_explicit_profile_read_and_preserves_selection_outside_catalog() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let read = request(&mut ws, "model.options", "work");
        assert_eq!(read["params"]["explicit_only"], true);
        assert!(read["params"].get("refresh").is_none());
        assert!(read["params"].get("include_unconfigured").is_none());
        answer(&mut ws, &read, payload());
    });
    let mut gateway = Gateway::connect(&Connection::Url {
        endpoint,
        token: None,
    })
    .unwrap();
    assert_eq!(
        gateway
            .model_catalog("work")
            .unwrap()
            .current_model
            .as_deref(),
        Some("unlisted-current-model")
    );
    server.join().unwrap();
}

#[test]
fn empty_rpc_catalog_recovers_through_same_owner_rest_without_losing_current_selection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/prefix", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let read = request(&mut ws, "model.options", "work");
        answer(
            &mut ws,
            &read,
            json!({"provider":"live-provider","model":"live-unlisted","providers":[]}),
        );
        let (mut http, _) = listener.accept().unwrap();
        let header = http_request(&mut http);
        let path = header
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
        assert_eq!(url.path(), "/prefix/api/model/options");
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query.get("profile").unwrap(), "work");
        assert_eq!(query.get("explicit_only").unwrap(), "1");
        assert_eq!(query.get("token").unwrap(), "fixture catalog token");
        http_answer(&mut http, payload());
    });
    let mut gateway = Gateway::connect(&Connection::Url {
        endpoint,
        token: Some("fixture catalog token".into()),
    })
    .unwrap();
    let catalog = gateway.model_catalog("work").unwrap();
    assert_eq!(catalog.current_provider.as_deref(), Some("live-provider"));
    assert_eq!(catalog.current_model.as_deref(), Some("live-unlisted"));
    assert_eq!(catalog.providers[0].models.len(), 3);
    server.join().unwrap();
}

#[test]
fn catalog_failure_remains_separate_from_subsequent_read_only_history() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let read = request(&mut ws, "model.options", "work");
        ws.send(Message::Text(
            json!({"id":read["id"],"error":{"code":4002,"message":"private server detail"}})
                .to_string()
                .into(),
        ))
        .unwrap();
        let (mut http, _) = listener.accept().unwrap();
        assert!(http_request(&mut http).starts_with("GET /api/model/options?profile=work&"));
        write!(http,"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 14\r\nConnection: close\r\n\r\nprivate-detail").unwrap();
        drop(http);
        let (mut http, _) = listener.accept().unwrap();
        assert!(http_request(&mut http).starts_with("GET /api/sessions/s/messages?profile=work&"));
        http_answer(
            &mut http,
            json!({"profile":"work","session_id":"s","messages":[{"role":"user","text":"Still readable"}]}),
        );
    });
    let mut gateway = Gateway::connect(&Connection::Url {
        endpoint,
        token: None,
    })
    .unwrap();
    let error = gateway.model_catalog("work").unwrap_err();
    assert!(!error.contains("private"));
    let History { messages, .. } = gateway.history("work", "s").unwrap();
    assert_eq!(messages[0].content, "Still readable");
    server.join().unwrap();
}
