//! The schema's Rust types against JSON: round trips, forward compatibility
//! and agreement with the JSON Schema.

use harness_core::*;

fn round_trip(line: &str) -> SessionEvent {
    let event: SessionEvent = serde_json::from_str(line).expect("parses");
    assert_eq!(serde_json::to_string(&event).unwrap(), line);
    event
}

#[test]
fn known_events_round_trip_byte_for_byte() {
    let e = round_trip(
        r#"{"session":"ses_1","seq":7,"at":1794646800000,"turn":"trn_1","step":"stp_1","type":"assistant","content":[{"type":"reasoning","text":"Short.","elapsed_ms":900},{"type":"text","text":"Hello."}],"stop":"end","stream":[[0,6,400],[1,6,1300]],"usage":{"input_tokens":120,"output_tokens":9}}"#,
    );
    assert_eq!(e.event.type_name(), "assistant");
    round_trip(
        r#"{"session":"ses_1","seq":8,"at":1794646800000,"turn":"trn_1","step":"stp_1","type":"tool.call","call":"cal_1","tool":"search","kind":"engine","input":{"limit":3,"q":"garden","ratio":0.5,"tags":["a",null,true,-2]},"batch":0}"#,
    );
}

#[test]
fn unknown_event_types_are_kept_generically() {
    let line = r#"{"session":"ses_1","seq":9,"at":1,"type":"future.thing","n":1,"x":{"y":[1,2]}}"#;
    let e = round_trip(line);
    match &e.event {
        Event::Unknown(u) => {
            assert_eq!(u.kind.0, "future.thing");
            assert_eq!(u.fields.len(), 2);
        }
        other => panic!("expected unknown, got {other:?}"),
    }
}

#[test]
fn a_malformed_known_event_is_an_error_not_unknown() {
    let line = r#"{"session":"ses_1","seq":9,"at":1,"type":"title","source":"model"}"#;
    assert!(serde_json::from_str::<SessionEvent>(line).is_err());
}

#[test]
fn unknown_enum_values_and_blocks_are_kept() {
    let e = round_trip(
        r#"{"session":"ses_1","seq":2,"at":1,"turn":"trn_1","type":"turn.start","cause":"handoff"}"#,
    );
    match e.event {
        Event::TurnStart(t) => assert_eq!(t.cause, Cause::Other("handoff".into())),
        other => panic!("{other:?}"),
    }
    let e = round_trip(
        r#"{"session":"ses_1","seq":3,"at":1,"turn":"trn_1","type":"user","content":[{"type":"image","blob":"blb_1"}]}"#,
    );
    match e.event {
        Event::User(u) => assert!(matches!(u.content[0], Block::Unknown(_))),
        other => panic!("{other:?}"),
    }
}

#[test]
fn headers_refuse_future_versions_and_other_formats() {
    assert!(LogHeader::current().check().is_ok());
    let future = LogHeader {
        version: SCHEMA_VERSION + 1,
        ..LogHeader::current()
    };
    assert_eq!(
        future.check(),
        Err(Error::FutureVersion {
            found: SCHEMA_VERSION + 1,
            supported: SCHEMA_VERSION
        })
    );
    let other = LogHeader {
        format: "something-else".into(),
        ..LogHeader::current()
    };
    assert!(other.check().is_err());
}

#[test]
fn the_json_schema_names_exactly_the_rust_types() {
    let schema: serde_json::Value = serde_json::from_str(JSON_SCHEMA).expect("valid JSON");
    let types: Vec<&str> = schema["properties"]["type"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(types, EVENT_TYPES);
    for name in EVENT_TYPES {
        let def = &schema["$defs"][*name];
        assert_eq!(def["properties"]["type"]["const"], *name, "$defs/{name}");
    }
    let blocks: Vec<&str> = schema["$defs"]["block"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["properties"]["type"]["const"].as_str().unwrap())
        .collect();
    assert_eq!(blocks, harness_core::block::BLOCK_TYPES);
}

#[test]
fn a_session_start_names_a_picked_model_only_when_there_is_one() {
    // A log written before the field existed: no `model`, and it encodes
    // back byte for byte.
    let old = round_trip(
        r#"{"session":"ses_1","seq":1,"at":1,"type":"session.start","class":"interactive","app":"chat","principal":{"kind":"user"},"preset":{"id":"chat","version":1},"profile":"standard"}"#,
    );
    match old.event {
        Event::SessionStart(s) => assert_eq!(s.model, None),
        other => panic!("{other:?}"),
    }
    let picked = round_trip(
        r#"{"session":"ses_1","seq":1,"at":1,"type":"session.start","class":"interactive","app":"chat","principal":{"kind":"user"},"preset":{"id":"chat","version":2},"profile":"standard","model":"local:picked"}"#,
    );
    match picked.event {
        Event::SessionStart(s) => assert_eq!(s.model.as_deref(), Some("local:picked")),
        other => panic!("{other:?}"),
    }
}
