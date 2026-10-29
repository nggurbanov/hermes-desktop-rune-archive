use super::*;
use serde_json::json;

fn scope(connection: u64, profile: &str, session: Option<&str>) -> DraftScope {
    DraftScope {
        connection,
        profile: profile.into(),
        session: session.map(String::from),
    }
}

fn catalog_value() -> serde_json::Value {
    json!({
        "provider":"Configured provider", "model":"reasoner",
        "providers":[{
            "slug":"configured", "name":"Configured provider", "authenticated":true,
            "models":["reasoner","plain","unavailable"], "unavailable_models":["unavailable"],
            "capabilities":{
                "reasoner":{"reasoning":true,"can_disable_reasoning":true,"fast":true,"ultrafast":false},
                "plain":{"reasoning":false,"fast":false,"ultrafast":false}
            }
        }]
    })
}

fn model(id: &str) -> ModelKey {
    ModelKey {
        provider: "configured".into(),
        id: id.into(),
    }
}

#[test]
fn draft_text_and_launch_choices_stay_with_the_connection_profile_and_session() {
    let mut composer = Composer::new();
    let first = scope(1, "default", Some("shared-id"));
    let others = [
        scope(2, "default", Some("shared-id")),
        scope(1, "work", Some("shared-id")),
        scope(1, "default", None),
    ];
    let draft = composer.draft_mut(first.clone());
    draft.text.replace_text("An unsent message");
    draft.reasoning = Some("high".into());
    draft.approval_mode = Some("smart".into());
    for other in &others {
        assert!(composer.draft_mut(other.clone()).text.text.is_empty());
        assert!(composer.draft(other).unwrap().approval_mode.is_none());
    }
    let restored = composer.draft(&first).unwrap();
    assert_eq!(restored.text.text, "An unsent message");
    assert_eq!(restored.reasoning.as_deref(), Some("high"));
    assert_eq!(restored.approval_mode.as_deref(), Some("smart"));
}

#[test]
fn selecting_models_respects_availability_and_drops_unsupported_launch_options() {
    let mut composer = Composer::new();
    let owner = scope(1, "default", Some("one"));
    composer
        .fixture_catalog(owner.clone(), &catalog_value())
        .unwrap();
    assert_eq!(
        composer.draft(&owner).unwrap().model,
        Some(model("reasoner"))
    );
    let draft = composer.draft_mut(owner.clone());
    draft.reasoning = Some("none".into());
    draft.service_tier = Some("priority".into());
    draft.approval_mode = Some("smart".into());
    composer
        .select_model(owner.clone(), model("plain"))
        .unwrap();
    let draft = composer.draft(&owner).unwrap();
    assert!(draft.reasoning.is_none());
    assert!(draft.service_tier.is_none());
    assert_eq!(draft.approval_mode.as_deref(), Some("smart"));

    composer.draft_mut(owner.clone()).reasoning = Some("none".into());
    composer.draft_mut(owner.clone()).service_tier = Some("ultrafast".into());
    composer
        .select_model(owner.clone(), model("reasoner"))
        .unwrap();
    assert_eq!(
        composer.draft(&owner).unwrap().reasoning.as_deref(),
        Some("none")
    );
    assert!(composer.draft(&owner).unwrap().service_tier.is_none());
    composer.toggle_popup(owner.clone(), Popup::Models);
    assert!(
        composer
            .select_model(owner.clone(), model("unavailable"))
            .is_err()
    );
    assert_eq!(
        composer.draft(&owner).unwrap().model,
        Some(model("reasoner"))
    );
    assert_eq!(composer.popup, Some(Popup::Models));
}

#[test]
fn stale_catalog_replies_cannot_cross_a_new_request_or_connection_owner() {
    let mut composer = Composer::new();
    let first = scope(1, "default", Some("one"));
    let current = scope(2, "default", Some("one"));
    composer.catalog_loading = true;
    composer.pending = Some(Pending {
        id: 2,
        scope: first.clone(),
    });
    assert!(!composer.apply_catalog_reply(
        CatalogReply {
            id: 1,
            scope: first.clone(),
            result: Ok(ModelCatalog::from_value(&catalog_value()).unwrap()),
        },
        &first
    ));
    assert!(composer.catalog_loading);
    assert!(composer.apply_catalog_reply(
        CatalogReply {
            id: 2,
            scope: first,
            result: Ok(ModelCatalog::from_value(&catalog_value()).unwrap()),
        },
        &current
    ));
    assert!(!composer.catalog_loading);
    assert!(composer.catalog_for(&current).is_none());
    assert!(composer.draft(&current).is_none());
}

#[test]
fn catalog_defaults_never_replace_an_explicit_choice_or_seed_a_background_draft() {
    let mut composer = Composer::new();
    let requested = scope(1, "default", Some("one"));
    let foreground = scope(1, "default", Some("two"));
    composer.pending = Some(Pending {
        id: 1,
        scope: requested.clone(),
    });
    composer.apply_catalog_reply(
        CatalogReply {
            id: 1,
            scope: requested,
            result: Ok(ModelCatalog::from_value(&catalog_value()).unwrap()),
        },
        &foreground,
    );
    assert!(composer.catalog_for(&foreground).is_some());
    assert!(composer.draft(&foreground).is_none());
    composer.toggle_popup(foreground.clone(), Popup::Models);
    assert_eq!(
        composer.draft(&foreground).unwrap().model,
        Some(model("reasoner"))
    );
    composer
        .select_model(foreground.clone(), model("plain"))
        .unwrap();
    composer.pending = Some(Pending {
        id: 2,
        scope: foreground.clone(),
    });
    composer.apply_catalog_reply(
        CatalogReply {
            id: 2,
            scope: foreground.clone(),
            result: Ok(ModelCatalog::from_value(&catalog_value()).unwrap()),
        },
        &foreground,
    );
    assert_eq!(
        composer.draft(&foreground).unwrap().model,
        Some(model("plain"))
    );
}

#[test]
fn favorites_are_client_preferences_bound_to_connection_and_provider() {
    let mut composer = Composer::new();
    composer.toggle_favorite(1, "first", "same-model");
    assert!(composer.is_favorite(1, "first", "same-model"));
    assert!(!composer.is_favorite(2, "first", "same-model"));
    assert!(!composer.is_favorite(1, "second", "same-model"));
    composer.toggle_favorite(1, "first", "same-model");
    assert!(!composer.is_favorite(1, "first", "same-model"));
}

#[test]
fn chosen_attachment_previews_are_local_and_removal_never_deletes_the_file() {
    let folder =
        std::env::temp_dir().join(format!("hermes-rune-composer-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let image = folder.join("chosen-preview.svg");
    std::fs::write(&image, r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><circle cx="10" cy="10" r="8" fill="blue"/></svg>"#).unwrap();
    let owner = scope(1, "default", Some("one"));
    let other = scope(1, "work", Some("one"));
    let mut composer = Composer::new();
    composer
        .fixture_catalog(owner.clone(), &catalog_value())
        .unwrap();
    composer
        .add_attachment(owner.clone(), image.clone())
        .unwrap();
    composer
        .add_attachment(owner.clone(), image.clone())
        .unwrap();
    let attachments = &composer.draft(&owner).unwrap().attachments;
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].name, "chosen-preview.svg");
    assert!(attachments[0].preview.as_ref().unwrap().wait().is_ok());
    assert!(composer.draft(&other).is_none());
    assert!(composer.remove_attachment(&owner, 0));
    assert!(image.is_file());
    let private_path = folder.join("private-missing-file.png");
    let error = composer.add_attachment(owner, private_path).unwrap_err();
    assert!(!error.contains("private-missing"));
    assert_eq!(composer.attachment_error.as_deref(), Some(error.as_str()));
    std::fs::remove_file(image).unwrap();
    std::fs::remove_dir(folder).unwrap();
}
