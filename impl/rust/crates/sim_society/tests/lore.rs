//! Task 4.1: the content director answers the living planet's requests from the recorded
//! responses in `content/lore/`, validated as the game's types; answers that break the rules
//! fall back.

use std::path::PathBuf;

use ai_gateway::{ContentDirector, ContentRequest, ReplayProvider, Source, Validate};
use serde_json::json;
use sim_society::{CodexEntry, CultureProfile, TechTier};

fn director() -> ContentDirector {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../content/lore");
    ContentDirector::new(Box::new(ReplayProvider::load_dir(dir).unwrap()))
}

fn culture(d: &ContentDirector, id: &str) -> ai_gateway::Answer<CultureProfile> {
    let request = ContentRequest::new(
        "culture_profile",
        format!("culture/{id}"),
        json!({"planet": "meridian"}),
    );
    d.request::<CultureProfile>(&request, || panic!("no profile for {id}"))
}

#[test]
fn both_cultures_come_from_the_recorded_responses_and_follow_the_rules() {
    let d = director();
    let hollin = culture(&d, "hollin");
    let compact = culture(&d, "compact");
    assert!(
        matches!(hollin.source, Source::Provider(_)),
        "{:?}",
        hollin.source
    );
    let (hollin, compact) = (hollin.value, compact.value);
    assert_eq!(hollin.tech_tier, TechTier::Medieval);
    assert_eq!(compact.tech_tier, TechTier::Spacefaring);
    assert_eq!(hollin.building_kit, "medieval_village");
    assert_eq!(compact.building_kit, "colony_standin");
    hollin.validate().unwrap();
    compact.validate().unwrap();
    for key in [
        "codex/culture/hollin",
        "codex/culture/compact",
        "codex/place/the-keel",
    ] {
        let a = d.request::<CodexEntry>(&ContentRequest::new("codex", key, json!({})), || {
            panic!("no codex entry {key}")
        });
        assert!(!a.value.body.is_empty());
    }
}

#[test]
fn a_profile_that_breaks_the_rules_falls_back() {
    let d = director();
    let mut bad = culture(&d, "hollin").value;
    // A medieval culture with spacefaring pilots and a colony's buildings.
    bad.roles[0].id = "pilot".into();
    bad.building_kit = "colony_standin".into();
    let problems = bad.validate().unwrap_err();
    assert!(
        problems.contains("pilot") && problems.contains("colony_standin"),
        "{problems}"
    );

    let text = json!({"responses": [{"request": {"kind": "culture_profile", "key": "culture/x"},
                                      "response": serde_json::to_value(&bad).unwrap()}]});
    let d = ContentDirector::new(Box::new(
        ReplayProvider::from_json("bad", &text.to_string()).unwrap(),
    ));
    let request = ContentRequest::new("culture_profile", "culture/x", json!({}));
    let fallback = culture(&director(), "hollin").value;
    let a = d.request::<CultureProfile>(&request, || fallback.clone());
    assert!(
        matches!(a.source, Source::Fallback(ref why) if why.contains("pilot")),
        "{:?}",
        a.source
    );
}
