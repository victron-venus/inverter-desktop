//! Read-only weather summaries from the already watched state payload.

use serde_json::Value;

use crate::state::MAX_TEXT_BYTES;
const MAX_CONDITION_BYTES: usize = 128;
const MAX_UNIT_BYTES: usize = 32;
const MAX_NUMBER_BYTES: usize = 32;
const MAX_FORECAST_ENTRIES: usize = 5;

fn plain(value: &str, limit: usize) -> Option<String> {
    let mut result = String::new();
    for character in value.chars().filter(|character| !character.is_control()) {
        if result.len() + character.len_utf8() > limit {
            return None;
        }
        result.push(character);
    }
    let result = result.trim();
    (!result.is_empty()).then(|| result.to_owned())
}

fn field(value: &Value, limit: usize) -> Option<String> {
    plain(value.as_str()?, limit)
}

fn temperature<'a, 'b>(value: &'a Value, unit: Option<&'b str>) -> Option<(&'a str, &'b str)> {
    let unit = unit?;
    let number = value.as_number()?;
    let literal = number.as_str();
    // Keep the parsed finite JSON token: float conversion would round values,
    // and truncating a long coefficient or exponent would change its meaning.
    if literal.len() > MAX_NUMBER_BYTES || number.as_f64().is_none() {
        return None;
    }
    Some((literal, unit))
}

fn digits(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0, |result, byte| {
        byte.is_ascii_digit()
            .then(|| result * 10 + u32::from(byte - b'0'))
    })
}

fn calendar_date(value: &[u8]) -> bool {
    if value.len() != 10 || value[4] != b'-' || value[7] != b'-' {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (
        digits(&value[..4]),
        digits(&value[5..7]),
        digits(&value[8..]),
    ) else {
        return false;
    };
    if year == 0 {
        return false;
    }
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

fn datetime(value: &Value) -> Option<&str> {
    let value = value.as_str()?;
    if value.len() > 35 || !value.is_ascii() {
        return None;
    }
    let (date, time) = value
        .split_once('T')
        .map_or((value, None), |(date, time)| (date, Some(time)));
    if !calendar_date(date.as_bytes()) {
        return None;
    }
    let Some(time) = time else {
        return Some(value);
    };
    let clock = time.as_bytes().get(..8)?;
    if clock[2] != b':'
        || clock[5] != b':'
        || digits(&clock[..2])? > 23
        || digits(&clock[3..5])? > 59
        || digits(&clock[6..])? > 59
    {
        return None;
    }
    let mut zone = &time[8..];
    if let Some(fraction) = zone.strip_prefix('.') {
        let length = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if !(1..=9).contains(&length) {
            return None;
        }
        zone = &fraction[length..];
    }
    if zone == "Z" {
        return Some(value);
    }
    let zone = zone.as_bytes();
    (zone.len() == 6
        && matches!(zone[0], b'+' | b'-')
        && zone[3] == b':'
        && digits(&zone[1..3]).is_some_and(|hours| hours <= 23)
        && digits(&zone[4..]).is_some_and(|minutes| minutes <= 59))
    .then_some(value)
}

fn append(text: &mut String, segment: &str, separator: &str) {
    if text.len() + separator.len() + segment.len() <= MAX_TEXT_BYTES {
        text.push_str(separator);
        text.push_str(segment);
    }
}

fn forecast(entry: &Value, unit: Option<&str>) -> Option<String> {
    let date = datetime(&entry["datetime"])?;
    let mut fields = Vec::new();
    if let Some(condition) = field(&entry["condition"], MAX_CONDITION_BYTES) {
        fields.push(format!("Condition: {condition}"));
    }
    for (key, label) in [("temperature", "High"), ("templow", "Low")] {
        if let Some((value, unit)) = temperature(&entry[key], unit) {
            fields.push(format!("{label}: {value} {unit}"));
        }
    }
    (!fields.is_empty()).then(|| format!("Forecast: {date}, {}", fields.join(", ")))
}

pub(crate) fn summary(condition: &str, attributes: &Value) -> Option<String> {
    let condition = plain(condition, MAX_CONDITION_BYTES)?;
    let mut text = format!("Condition: {condition}");
    let unit = field(&attributes["temperature_unit"], MAX_UNIT_BYTES);
    if let Some((value, unit)) = temperature(&attributes["temperature"], unit.as_deref()) {
        append(&mut text, &format!("Temperature: {value} {unit}"), "; ");
    }
    if let Some(entries) = attributes["forecast"].as_array() {
        // New HA forecast APIs are deliberately outside this state projection.
        for entry in entries.iter().take(MAX_FORECAST_ENTRIES) {
            if let Some(segment) = forecast(entry, unit.as_deref()) {
                append(&mut text, &segment, "\n");
            }
        }
    }
    Some(text)
}

pub(crate) fn structured(condition: &str, attributes: &Value) -> Option<Value> {
    let condition = plain(condition, MAX_CONDITION_BYTES)?;
    let unit =
        field(&attributes["temperature_unit"], MAX_UNIT_BYTES).unwrap_or_else(|| "°C".into());
    let current =
        temperature(&attributes["temperature"], Some(&unit)).map_or("", |(value, _)| value);
    let forecast = attributes["forecast"].as_array().into_iter().flatten().take(MAX_FORECAST_ENTRIES)
        .filter_map(|entry| {
            let date = datetime(&entry["datetime"])?;
            let mut item = serde_json::json!({"datetime":date,
                "condition":field(&entry["condition"], MAX_CONDITION_BYTES).unwrap_or_default(),
                "temperature":temperature(&entry["temperature"], Some(&unit)).map_or("", |(value, _)| value)});
            if let Some((low, _)) = temperature(&entry["templow"], Some(&unit)) { item["templow"] = serde_json::json!(low); }
            Some(item)
        }).collect::<Vec<_>>();
    Some(
        serde_json::json!({"condition":condition,"temperature":current,"unit":unit,"forecast":forecast}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_weather_unit_and_exact_numeric_tokens_are_preserved() {
        let attributes = serde_json::from_str(
            r#"{"temperature":21.500,"temperature_unit":"°C","unit_of_measurement":"wrong"}"#,
        )
        .unwrap();
        assert_eq!(
            summary("sunny", &attributes).unwrap(),
            "Condition: sunny; Temperature: 21.500 °C"
        );
        let attributes = json!({"temperature":-4,"temperature_unit":"°F"});
        assert_eq!(
            summary("snowy", &attributes).unwrap(),
            "Condition: snowy; Temperature: -4 °F"
        );
    }

    #[test]
    fn temperatures_need_finite_json_numbers_and_an_explicit_nonempty_unit() {
        for value in [json!("21"), json!(true), json!(null), json!([]), json!({})] {
            assert_eq!(
                summary(
                    "cloudy",
                    &json!({"temperature":value,"temperature_unit":"°C"})
                )
                .unwrap(),
                "Condition: cloudy"
            );
        }
        for unit in [
            json!(null),
            json!(true),
            json!(17),
            json!(""),
            json!("\n\t"),
            json!("x".repeat(33)),
        ] {
            assert_eq!(
                summary(
                    "cloudy",
                    &json!({"temperature":21,"temperature_unit":unit,"unit_of_measurement":"°C"})
                )
                .unwrap(),
                "Condition: cloudy"
            );
        }
        assert_eq!(
            summary(
                "cloudy",
                &json!({"temperature":21,"unit_of_measurement":"°C"})
            )
            .unwrap(),
            "Condition: cloudy"
        );
    }

    #[test]
    fn arbitrary_precision_overflow_and_oversized_tokens_are_not_truncated_or_rounded() {
        for literal in [
            "1e999".to_owned(),
            "-1e999".to_owned(),
            "9".repeat(300),
            format!("0.{}1", "0".repeat(8192)),
        ] {
            let attributes: Value = serde_json::from_str(&format!(
                r#"{{"temperature":{literal},"temperature_unit":"K"}}"#
            ))
            .unwrap();
            assert_eq!(summary("sunny", &attributes).unwrap(), "Condition: sunny");
        }
        for (literal, expected) in [
            ("1e300", "1e+300"),
            ("1e-999", "1e-999"),
            (
                "12345678901234567890123456789012",
                "12345678901234567890123456789012",
            ),
            (
                "-0.00000000000000000000000000001",
                "-0.00000000000000000000000000001",
            ),
        ] {
            let attributes: Value = serde_json::from_str(&format!(
                r#"{{"temperature":{literal},"temperature_unit":"K"}}"#
            ))
            .unwrap();
            assert_eq!(
                summary("sunny", &attributes).unwrap(),
                format!("Condition: sunny; Temperature: {expected} K")
            );
        }
    }

    #[test]
    fn legacy_forecast_uses_only_supplied_dates_conditions_and_temperatures() {
        let attributes = json!({"temperature":21,"temperature_unit":"°C","forecast":[
            {"datetime":"2028-02-29T12:34:56.123456789+05:30","condition":"rainy","temperature":23,"templow":14},
            {"datetime":"2026-09-16","condition":"cloudy"},
            {"datetime":"2026-09-17T00:00:00Z","temperature":19.5,"templow":"not a number"}
        ]});
        assert_eq!(summary("sunny", &attributes).unwrap(),
            "Condition: sunny; Temperature: 21 °C\nForecast: 2028-02-29T12:34:56.123456789+05:30, Condition: rainy, High: 23 °C, Low: 14 °C\nForecast: 2026-09-16, Condition: cloudy\nForecast: 2026-09-17T00:00:00Z, High: 19.5 °C");
        assert_eq!(summary("sunny", &json!({"forecast":[{"datetime":"2026-09-16","condition":"cloudy","temperature":23,"templow":14}]})).unwrap(),
            "Condition: sunny\nForecast: 2026-09-16, Condition: cloudy");
    }

    #[test]
    fn invalid_calendar_dates_times_and_shapes_never_create_forecast_labels() {
        for value in [
            json!(null),
            json!(17),
            json!({}),
            json!(""),
            json!("2026-02-29"),
            json!("1900-02-29"),
            json!("0000-01-01"),
            json!("2026-00-01"),
            json!("2026-04-31"),
            json!("2026-12-00"),
            json!("2026-12-32"),
            json!("2026-09-16T24:00:00Z"),
            json!("2026-09-16T12:60:00Z"),
            json!("2026-09-16T12:00:60Z"),
            json!("2026-09-16T12:00:00"),
            json!("2026-09-16T12:00:00.Z"),
            json!("2026-09-16T12:00:00.1234567890Z"),
            json!("2026-09-16T12:00:00+24:00"),
            json!("2026-09-16T12:00:00+01:60"),
            json!("2026-09-16T12:00:00+0100"),
            json!("2026-09-16T12:00:00Zsuffix"),
            json!("2026-09-16\n"),
            json!("2026-09-16T12:00:00Z\t"),
            json!("☀️2026-09-16"),
            json!("2026-09-16T☀️:00:00Z"),
        ] {
            assert_eq!(
                summary(
                    "sunny",
                    &json!({"forecast":[{"datetime":value,"condition":"rainy"}]})
                )
                .unwrap(),
                "Condition: sunny",
                "rejected date {value}"
            );
        }
        assert!(datetime(&json!("2000-02-29T23:59:59-23:59")).is_some());
    }

    #[test]
    fn forecast_examines_only_the_first_five_source_entries_and_skips_malformed_fields() {
        let mut entries = vec![json!(null); 5];
        entries.push(json!({"datetime":"2026-09-16","condition":"must not appear"}));
        assert_eq!(
            summary("sunny", &json!({"forecast":entries})).unwrap(),
            "Condition: sunny"
        );
        let attributes = json!({"temperature_unit":"°C","forecast":[
            null, [], {"datetime":"2026-09-16"}, {"condition":"cloudy"},
            {"datetime":"2026-09-16","condition":false,"temperature":false,"templow":12}
        ]});
        assert_eq!(
            summary("sunny", &attributes).unwrap(),
            "Condition: sunny\nForecast: 2026-09-16, Low: 12 °C"
        );
        for forecast in [json!(null), json!(true), json!("forecast"), json!({})] {
            assert_eq!(
                summary("sunny", &json!({"forecast":forecast})).unwrap(),
                "Condition: sunny"
            );
        }
    }

    #[test]
    fn strings_are_utf8_bounded_and_controls_cannot_inject_lines_or_partial_fields() {
        assert_eq!(
            summary(
                " \nsun\tny\u{7f} ",
                &json!({"temperature":21,"temperature_unit":"\n°C\t"})
            )
            .unwrap(),
            "Condition: sunny; Temperature: 21 °C"
        );
        assert_eq!(
            summary(&"☀".repeat(42), &json!({})).unwrap(),
            format!("Condition: {}", "☀".repeat(42))
        );
        assert!(summary(
            &"☀".repeat(43),
            &json!({"temperature":21,"temperature_unit":"°C"})
        )
        .is_none());
        assert!(summary(" \r\n\t", &json!({})).is_none());
        let attributes = json!({"temperature":21,"temperature_unit":"é".repeat(17),"forecast":[{"datetime":"2026-09-16","condition":"x".repeat(129),"temperature":23}]});
        assert_eq!(summary("sunny", &attributes).unwrap(), "Condition: sunny");
    }

    #[test]
    fn forecast_entries_fit_as_whole_segments_and_never_expose_partial_numbers() {
        let forecast = json!({"datetime":"2026-09-16T12:00:00+00:00","condition":"\\\"".repeat(64),"temperature":12345678901234567890123456789012_i128,"templow":-4});
        let mut attributes =
            json!({"temperature":21,"temperature_unit":"°C","forecast":vec![forecast;5]});
        let oversized = summary("sunny", &attributes).unwrap();
        assert_eq!(oversized, "Condition: sunny; Temperature: 21 °C");
        for entry in attributes["forecast"].as_array_mut().unwrap() {
            entry["condition"] = json!("\\\"".repeat(32));
        }
        let text = summary("sunny", &attributes).unwrap();
        assert!(text.len() <= MAX_TEXT_BYTES);
        assert_eq!(text.matches("Forecast:").count(), 1);
        assert!(text.ends_with("High: 12345678901234567890123456789012 °C, Low: -4 °C"));
    }
}
