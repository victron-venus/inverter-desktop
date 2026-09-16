use super::*;

fn configuration(values: Value) -> Validated {
    let mut values = values;
    values["ha_base_url"] = json!("http://localhost/reverse/ha/");
    serde_json::from_value::<crate::config::Configuration>(
        json!({"revision":"presentation-test", "values":values,"secrets":{"ha_token":"fixture"}}),
    )
    .unwrap()
    .validate()
    .unwrap()
}

fn state(entity: &str, value: &str) -> Value {
    json!({"entity_id":entity,"state":value,"attributes":{"friendly_name":"Identical title"}})
}

fn presentation<'a>(frame: &'a Value, id: &str) -> &'a Value {
    frame["presentation"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap()
}

#[test]
fn exact_placements_labels_and_primary_target_identity_survive_duplicate_titles() {
    let layout = json!({"version":1,"controls":[
        {"id":"header-fan","surface":"header","order":4,"label":"My fan","entity":"fan.study","icon":"plug"},
        {"id":"home-fan","surface":"home","order":2,"label":"Same fan","entity":"fan.study","icon":"home"},
        {"id":"home-other","surface":"home","order":0,"label":"Other","entity":"switch.study","icon":"light"}
    ]});
    let config = configuration(json!({"dashboard_layout":layout.to_string()}));
    assert_eq!(config.entities, ["fan.study", "switch.study"]);
    assert_eq!(config.actions().len(), 2);
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    book.initial("fan.study", Some(&state("fan.study", "on")));
    book.initial("switch.study", Some(&state("switch.study", "off")));
    let frame = book.frame();
    assert_eq!(
        presentation(&frame, "header-fan"),
        &json!({"kind":"control","id":"header-fan","surface":"header","order":4,"title":"My fan","icon":"plug","state":"on","action":"ha-primary-0"})
    );
    assert_eq!(presentation(&frame, "home-fan")["action"], "ha-primary-0");
    assert_eq!(presentation(&frame, "home-other")["action"], "ha-primary-1");
    for (value, expected) in [
        (" UNLOCKED ", "on"),
        ("opening", "on"),
        ("playing", "off"),
        ("true", "off"),
        ("1", "off"),
        ("unknown", "unavailable"),
        ("", "unavailable"),
    ] {
        book.live("fan.study", Some(&state("fan.study", value)));
        assert_eq!(presentation(&book.frame(), "header-fan")["state"], expected);
    }
    book.disconnected();
    let frame = book.frame();
    assert_eq!(presentation(&frame, "ha-connection")["connected"], false);
    assert_eq!(presentation(&frame, "header-fan")["state"], "unavailable");
    assert!(presentation(&frame, "header-fan").get("action").is_none());
}

#[test]
fn unknown_primary_controls_keep_exact_grants_without_authorizing_unavailable_or_disconnected() {
    let targets = ["button.start", "scene.evening", "switch.room"];
    let controls: Vec<_> = targets
        .iter()
        .enumerate()
        .map(|(index, entity)| {
            json!({"id":format!("home-{index}"),"surface":"home","order":index,
            "label":"Configured control","entity":entity,"icon":"plug"})
        })
        .collect();
    let config = configuration(
        json!({"dashboard_layout":json!({"version":1,"controls":controls}).to_string()}),
    );
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    for target in targets {
        book.initial(target, Some(&state(target, "unknown")));
    }
    let frame = book.frame();
    book.mark_published();
    for (index, target) in targets.iter().enumerate() {
        let control = presentation(&frame, &format!("home-{index}"));
        let action_id = format!("ha-primary-{index}");
        assert_eq!(
            control["state"], "unavailable",
            "unknown stays visually distinct"
        );
        assert_eq!(control["action"], action_id);
        assert_eq!(book.action_target(&action_id).unwrap().entity, *target);
    }
    for target in targets {
        book.live(target, Some(&state(target, "unavailable")));
    }
    let frame = book.frame();
    for index in 0..targets.len() {
        assert!(presentation(&frame, &format!("home-{index}"))
            .get("action")
            .is_none());
        assert!(book.action_target(&format!("ha-primary-{index}")).is_none());
    }
    for target in targets {
        book.live(target, Some(&state(target, "unknown")));
    }
    book.mark_published();
    assert!(book.action_target("ha-primary-0").is_some());
    book.disconnected();
    let frame = book.frame();
    for index in 0..targets.len() {
        assert!(presentation(&frame, &format!("home-{index}"))
            .get("action")
            .is_none());
        assert!(book.action_target(&format!("ha-primary-{index}")).is_none());
    }
}

#[test]
fn appliance_activity_visibility_complete_time_and_independent_start_pause_refs_match_legacy() {
    let layout = json!({"version":1,"sections":{"dryer":false},"appliances":{"washer_start":"button.start","dryer_pause":"button.pause"}});
    let config = configuration(
        json!({"dashboard_layout":layout.to_string(),"dishwasher_running_entity":"binary_sensor.running","dishwasher_duration_entity":"sensor.duration","washer_remaining_entity":"sensor.washer","dryer_remaining_entity":"sensor.dryer"}),
    );
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    book.initial("button.start", Some(&state("button.start", "unknown")));
    book.initial("button.pause", None);
    book.initial("sensor.duration", Some(&state("sensor.duration", "1.500")));
    book.initial(
        "binary_sensor.running",
        Some(&state("binary_sensor.running", " RUNNING ")),
    );
    book.initial("sensor.dryer", Some(&state("sensor.dryer", "01:20:00")));
    for (time, active) in [
        ("0", false),
        ("00:00", false),
        ("idle", false),
        ("unknown", false),
        ("unavailable", false),
        ("paused", false),
        ("01:00", true),
        ("2h 10m", true),
    ] {
        book.live("sensor.washer", Some(&state("sensor.washer", time)));
        let frame = book.frame();
        let washer = presentation(&frame, "ha-washer");
        assert_eq!(washer["active"], active, "{time}");
        assert_eq!(washer["visible"], active);
        assert_eq!(washer["actions"].as_array().unwrap().len(), 1);
        assert_eq!(washer["actions"][0]["label"], "Start");
        assert_eq!(presentation(&frame, "ha-dryer")["active"], true);
        assert_eq!(presentation(&frame, "ha-dryer")["visible"], false);
        assert!(presentation(&frame, "ha-dryer")["actions"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(presentation(&frame, "ha-dishwasher")["text"], "1.500");
    }
    for (value, active) in [
        ("ON", true),
        (" running ", true),
        ("true", false),
        ("off", false),
        ("unknown", false),
    ] {
        book.live(
            "binary_sensor.running",
            Some(&state("binary_sensor.running", value)),
        );
        assert_eq!(
            presentation(&book.frame(), "ha-dishwasher")["active"],
            active
        );
    }
}

#[test]
fn groups_and_weather_come_from_typed_observations_and_only_current_exact_grants() {
    let config = configuration(json!({"dashboard_layout":json!({"version":1}).to_string(),
        "watch_entities":"sensor.a,number.a,cover.a,media_player.a,scene.a,weather.a",
        "number_entities":"number.a","media_player_entities":"media_player.a","action_entities":"scene.a","cover_position_entities":"cover.a"}));
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    for name in &config.entities {
        book.initial(name, Some(&state(name, "on")));
    }
    let mut number = state("number.a", "1.50");
    number["attributes"] = json!({"min":0,"max":10,"step":0.5,"unit_of_measurement":"kW"});
    book.live("number.a", Some(&number));
    let mut weather = state("weather.a", "sunny");
    weather["attributes"] = json!({"temperature":21.5,"temperature_unit":"°C","forecast":[{"datetime":"2026-09-16","temperature":23,"templow":14,"condition":"rainy"}]});
    book.live("weather.a", Some(&weather));
    let frame = book.frame();
    assert_eq!(presentation(&frame, "ha-weather")["temperature"], "21.5");
    assert_eq!(
        presentation(&frame, "ha-weather")["forecast"][0]["templow"],
        "14"
    );
    let numbers = presentation(&frame, "ha-numbers");
    assert_eq!(numbers["collapsed"], true);
    assert_eq!(numbers["rows"][0]["value"], "entity-1");
    assert_eq!(numbers["rows"][0]["input"], "ha-number-0-set");
    assert_eq!(
        presentation(&frame, "ha-media")["rows"][0]["actions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    book.live("media_player.a", None);
    book.live("number.a", None);
    let next = book.frame();
    assert!(!next["presentation"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["id"] == "ha-media" || entry["id"] == "ha-numbers"));
    book.live("cover.a", None);
    assert_eq!(
        presentation(&book.frame(), "ha-covers")["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn catalog_pages_are_bounded_atomic_read_only_and_include_all_supported_domains() {
    let config = configuration(
        json!({"dashboard_layout":json!({"version":1}).to_string(),"discovery_domains":"sensor,weather,fan,cover,number"}),
    );
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.begin_session();
    book.connected();
    let states: Vec<_> = (0..65)
        .map(|index| state(&format!("sensor.catalog_{index:03}"), "1"))
        .chain([
            state("fan.office", "off"),
            state("weather.house", "cloudy"),
            state("cover.blind", "open"),
            state("number.limit", "10"),
            state("unsupported.other", "on"),
        ])
        .collect();
    book.discovery_snapshot(&states);
    let mut offset = 0;
    for (index, frame) in book.catalog.iter().enumerate() {
        assert_eq!(frame["data"]["offset"], offset);
        let options = frame["data"]["options"].as_array().unwrap();
        assert!(options.len() <= 64);
        assert!(serde_json::to_vec(&frame["data"]).unwrap().len() <= 16 * 1024);
        offset += options.len();
        assert_eq!(frame["data"]["complete"], index + 1 == book.catalog.len());
    }
    assert_eq!(offset, 69);
    let frame = book.frame();
    assert!(!frame["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["kind"] == "action" || item["kind"] == "number_input"));
    assert!(frame["items"].as_array().unwrap().len() <= 65);
    assert!(book.catalog.iter().any(|page| page["data"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .any(|option| option["value"] == "weather.house")));
}

#[test]
fn maximum_compact_read_control_and_projection_frame_fits_the_real_wire_limit() {
    // Maximize escaped labels within the actual 32KiB configuration boundary.
    let config = (1..=64).rev().find_map(|repeats| {
        let controls:Vec<_>=(0..63).map(|index|json!({"id":format!("control-{index}-{}","x".repeat(49)),"surface":"home","order":index,
            "label":"\\\"".repeat(repeats),"entity":format!("media_player.{}_{index}","x".repeat(106)),"icon":"plug"})).collect();
        serde_json::from_value::<crate::config::Configuration>(json!({"revision":"maximum-layout","values":{
            "ha_base_url":"http://localhost/","dashboard_layout":json!({"version":1,"controls":controls}).to_string(),"watch_entities":"sensor.readonly"},"secrets":{"ha_token":"fixture"}})).unwrap().validate().ok()
    }).expect("bounded maximum layout");
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    for entity in &config.entities {
        let mut value = state(entity, &"\\\"".repeat(128));
        value["attributes"]["friendly_name"] = json!("\\\"".repeat(64));
        book.initial(entity, Some(&value));
    }
    let frame = book.frame();
    let bytes = serde_json::to_vec(&frame).unwrap();
    assert_eq!(frame["items"].as_array().unwrap().len(), 128);
    assert!(
        bytes.len() < inverter_worker_protocol::MAX_FRAME_BYTES,
        "{} bytes",
        bytes.len()
    );
    assert!(frame["presentation"].as_array().unwrap().len() <= 96);
}

#[test]
fn malformed_layouts_and_unsupported_discovery_are_rejected_before_authority() {
    let base = json!({"version":1,"controls":[{"id":"my-control","surface":"home","order":0,"label":"My light","entity":"light.a","icon":"light"}]});
    for (field, invalid) in [
        ("id", json!("ha-connection")),
        ("id", json!("invalid:id")),
        ("surface", json!("sidebar")),
        ("entity", json!("weather.a")),
        ("label", json!("")),
        ("label", json!("x".repeat(129))),
        ("icon", json!("remote-icon")),
    ] {
        let mut layout = base.clone();
        layout["controls"][0][field] = invalid;
        assert!(crate::presentation::Layout::parse(&layout.to_string()).is_err());
    }
    for layout in [
        json!({"version":2}),
        json!({"version":1,"sections":{"unknown":true}}),
        json!({"version":1,"appliances":{"washer_start":"weather.a"}}),
        json!({"version":1,"controls":[base["controls"][0].clone(),base["controls"][0].clone()]}),
    ] {
        assert!(crate::presentation::Layout::parse(&layout.to_string()).is_err());
    }
    assert!(crate::presentation::Layout::parse(&" ".repeat(24577)).is_err());
    for domains in [
        "alarm_control_panel",
        "sensor.*",
        "Sensor",
        &" ".repeat(257),
    ] {
        let value=serde_json::from_value::<crate::config::Configuration>(json!({"revision":"bad-domain","values":{"ha_base_url":"http://localhost","discovery_domains":domains},"secrets":{"ha_token":"fixture"}})).unwrap();
        assert!(value.validate().is_err());
    }
}

#[test]
fn each_action_family_can_use_the_shared_budget_and_never_silently_drops_targets() {
    for (field, domain, count, cost) in [
        ("action_entities", "button", 63, 1),
        ("media_player_entities", "media_player", 21, 3),
        ("binary_entities", "switch", 31, 2),
        ("cover_entities", "cover", 21, 3),
        ("number_entities", "number", 63, 1),
        ("cover_position_entities", "cover", 63, 1),
    ] {
        let selected = (0..count)
            .map(|i| format!("{domain}.{}_{i:02}", "x".repeat(128 - domain.len() - 4)))
            .collect::<Vec<_>>();
        assert!(selected.iter().all(|entity| entity.len() == 128));
        let config = configuration(json!({field:selected.join(",")}));
        assert_eq!(config.entities.len(), count);
        assert_eq!(config.actions().len() + config.inputs().len(), count * cost);
        let mut value = json!({"revision":"overflow","values":{},"secrets":{"ha_token":"fixture"}});
        value["values"]["ha_base_url"] = json!("http://localhost");
        value["values"][field] = json!((0..count + 1)
            .map(|i| format!("{domain}.e{i}"))
            .collect::<Vec<_>>()
            .join(","));
        assert!(
            serde_json::from_value::<crate::config::Configuration>(value)
                .unwrap()
                .validate()
                .is_err()
        );
    }
}

#[test]
fn compact_numeric_display_keeps_literal_precision_and_notification_titles() {
    let config = configuration(
        json!({"dashboard_layout":json!({"version":1}).to_string(),"watch_entities":"sensor.power,switch.desk","notify_home":true}),
    );
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    let mut power = state("sensor.power", "1.500");
    power["attributes"]["unit_of_measurement"] = json!("kW");
    book.initial("sensor.power", Some(&power));
    book.initial(
        "switch.desk",
        Some(&json!({"entity_id":"switch.desk","state":"off","attributes":{}})),
    );
    book.live(
        "switch.desk",
        Some(&json!({"entity_id":"switch.desk","state":"on","attributes":{}})),
    );
    assert_eq!(book.notifications.next().unwrap()["body"], "desk: ON");
    assert_eq!(book.frame()["items"][1]["text"], "1.500 kW");
}

#[test]
fn appliance_start_pause_can_share_one_explicit_target_without_duplicate_authority() {
    let config = configuration(
        json!({"dashboard_layout":json!({"version":1,"appliances":{"washer_start":"button.shared","washer_pause":"button.shared"}}).to_string(),"washer_remaining_entity":"sensor.remaining"}),
    );
    assert_eq!(config.actions().len(), 1);
    let shared = Book::configured(&config);
    let mut book = shared.lock().unwrap();
    book.connected();
    book.initial(
        "sensor.remaining",
        Some(&state("sensor.remaining", "01:20")),
    );
    book.initial("button.shared", Some(&state("button.shared", "unknown")));
    let frame = book.frame();
    assert_eq!(
        presentation(&frame, "ha-washer")["actions"],
        json!([{"id":"ha-primary-0","label":"Start"},{"id":"ha-primary-0","label":"Pause"}])
    );
}

#[test]
fn expanded_numeric_and_cover_capacities_remain_inside_compact_and_flat_frames() {
    for layout in ["", "{\"version\":1}"] {
        for (field, domain, count, cost) in [
            ("number_entities", "number", 63, 1),
            ("cover_position_entities", "cover", 63, 1),
            ("cover_entities", "cover", 21, 3),
        ] {
            let targets = (0..count)
                .map(|index| format!("{domain}.n{index}"))
                .collect::<Vec<_>>();
            let mut values = json!({field:targets.join(","),"dashboard_layout":layout});
            values["watch_entities"] = json!((0..64 - count)
                .map(|index| format!("sensor.other{index}"))
                .collect::<Vec<_>>()
                .join(","));
            let config = configuration(values);
            let shared = Book::configured(&config);
            let mut book = shared.lock().unwrap();
            book.connected();
            for entity in &config.entities {
                let mut value = state(
                    entity,
                    if domain == "cover" && entity.starts_with("cover.") {
                        "open"
                    } else {
                        "1.500"
                    },
                );
                value["attributes"] = json!({"friendly_name":"\"".repeat(128),"unit_of_measurement":"\"".repeat(32),"min":0,"max":100,"step":0.5,"supported_features":15,"current_position":50});
                book.initial(entity, Some(&value));
            }
            let frame = book.frame();
            assert_eq!(frame["items"].as_array().unwrap().len(), 65 + count * cost);
            assert!(
                serde_json::to_vec(&frame).unwrap().len()
                    < inverter_worker_protocol::MAX_FRAME_BYTES,
                "{field}, compact={}",
                !layout.is_empty()
            );
        }
    }
}

#[test]
fn clearing_an_optional_appliance_picker_removes_its_grant() {
    let config = configuration(
        json!({"dashboard_layout":json!({"version":1,"appliances":{"washer_start":"","washer_pause":"   "}}).to_string()}),
    );
    assert!(config.entities.is_empty());
    assert!(config.actions().is_empty());
    assert!(config.layout.unwrap().appliances.is_empty());
    assert!(
        crate::presentation::Layout::parse(r#"{"version":1,"appliances":{"unknown":""}}"#).is_err()
    );
}

#[test]
fn omitted_layout_enables_compact_default_while_explicit_empty_keeps_flat_optout() {
    let base = json!({"revision":"default-layout","values":{"ha_base_url":"http://localhost"},"secrets":{"ha_token":"fixture"}});
    let parsed = serde_json::from_value::<crate::config::Configuration>(base.clone()).unwrap();
    assert_eq!(
        parsed.values.dashboard_layout,
        crate::config::DEFAULT_DASHBOARD_LAYOUT
    );
    assert!(serde_json::to_value(&parsed).unwrap()["values"]
        .get("dashboard_layout")
        .is_none());
    let default = parsed.validate().unwrap();
    let shared = Book::configured(&default);
    assert_eq!(
        shared.lock().unwrap().frame()["presentation"][0]["kind"],
        "connection"
    );
    let mut explicit = base;
    explicit["values"]["dashboard_layout"] = json!("");
    let parsed = serde_json::from_value::<crate::config::Configuration>(explicit).unwrap();
    assert_eq!(
        serde_json::to_value(&parsed).unwrap()["values"]["dashboard_layout"],
        ""
    );
    let config = parsed.validate().unwrap();
    assert!(config.layout.is_none());
    let shared = Book::configured(&config);
    assert!(shared.lock().unwrap().frame().get("presentation").is_none());
}
