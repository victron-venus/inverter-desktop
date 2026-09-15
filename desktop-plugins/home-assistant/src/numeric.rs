//! Exact, bounded decimal observations and numeric service arguments.

use crate::config::Operation;
use serde_json::{Number, Value};

pub const MAX_SCALED: i64 = 1_000_000_000_000_000;
const MAX_LITERAL_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Constraints {
    pub min: i64,
    pub max: i64,
    pub step: i64,
    pub decimal_places: u8,
    pub unit: Option<String>,
}

impl Constraints {
    pub fn accepts(&self, value: i64) -> bool {
        (-MAX_SCALED..=MAX_SCALED).contains(&value)
            && (self.min..=self.max).contains(&value)
            && (value - self.min) % self.step == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub constraints: Constraints,
    pub value: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Decimal {
    coefficient: i64,
    places: u8,
}

impl Decimal {
    fn at(self, places: u8) -> Option<i64> {
        let value = self
            .coefficient
            .checked_mul(10_i64.pow(u32::from(places.checked_sub(self.places)?)))?;
        (-MAX_SCALED..=MAX_SCALED).contains(&value).then_some(value)
    }
}

// Parse the original decimal representation, including bounded exponents. Never
// pass metadata through f64: rounding could manufacture an eligible grid value.
fn decimal(literal: &str) -> Option<Decimal> {
    if literal.is_empty() || literal.len() > MAX_LITERAL_BYTES {
        return None;
    }
    let (negative, unsigned) = literal
        .strip_prefix('-')
        .map_or((false, literal), |v| (true, v));
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => {
            let digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (mantissa, exponent.parse::<i32>().ok()?)
        }
        None => (unsigned, 0),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (mantissa, ""),
    };
    if whole.is_empty()
        || (whole.len() > 1 && whole.starts_with('0'))
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Some(Decimal {
            coefficient: 0,
            places: 0,
        });
    }
    let normalized = significant.trim_end_matches('0');
    let trailing = significant.len() - normalized.len();
    let places = i32::try_from(fraction.len())
        .ok()?
        .checked_sub(exponent)?
        .checked_sub(i32::try_from(trailing).ok()?)?;
    if places > 6 || normalized.len() > 16 || places < -15 {
        return None;
    }
    let mut coefficient = normalized.parse::<i64>().ok()?;
    if places < 0 {
        coefficient = coefficient.checked_mul(10_i64.pow(places.unsigned_abs()))?;
    }
    if coefficient > MAX_SCALED {
        return None;
    }
    Some(Decimal {
        coefficient: if negative { -coefficient } else { coefficient },
        places: places.max(0) as u8,
    })
}

fn attribute(value: &Value) -> Option<Decimal> {
    decimal(&value.as_number()?.to_string())
}

fn number(state: &Value) -> Option<Observation> {
    let value = decimal(state["state"].as_str()?)?;
    let attrs = &state["attributes"];
    let min = attribute(&attrs["min"])?;
    let max = attribute(&attrs["max"])?;
    let step = attribute(&attrs["step"])?;
    // Precision belongs to the constraints. A state requiring finer precision
    // cannot silently change the grant or invalidate an ordinary edited draft.
    let places = min.places.max(max.places).max(step.places);
    let unit = match attrs.get("unit_of_measurement") {
        None | Some(Value::Null) => None,
        Some(Value::String(unit)) => {
            let unit = crate::state::bounded(unit, 32);
            (!unit.is_empty()).then_some(unit)
        }
        Some(_) => return None,
    };
    let constraints = Constraints {
        min: min.at(places)?,
        max: max.at(places)?,
        step: step.at(places)?,
        decimal_places: places,
        unit,
    };
    if constraints.min > constraints.max || constraints.step <= 0 {
        return None;
    }
    let value = value.at(places)?;
    constraints
        .accepts(value)
        .then_some(Observation { constraints, value })
}

fn cover_position(state: &Value) -> Option<Observation> {
    if !matches!(
        state["state"].as_str(),
        Some("open" | "closed" | "opening" | "closing")
    ) || state["attributes"]["supported_features"].as_u64()? & 4 == 0
    {
        return None;
    }
    let value = state["attributes"]["current_position"].as_i64()?;
    let constraints = Constraints {
        min: 0,
        max: 100,
        step: 1,
        decimal_places: 0,
        unit: Some("%".into()),
    };
    constraints
        .accepts(value)
        .then_some(Observation { constraints, value })
}

pub fn observe(operation: Operation, entity: &str, state: Option<&Value>) -> Option<Observation> {
    let state = state.filter(|state| state["entity_id"].as_str() == Some(entity))?;
    match operation {
        Operation::SetNumber => number(state),
        Operation::SetCoverPosition => cover_position(state),
        _ => None,
    }
}

pub fn params(params: &Value) -> Option<(&str, i64)> {
    let object = params.as_object()?;
    if object.len() != 2 {
        return None;
    }
    let revision = object.get("input_revision")?.as_str()?;
    let value = object.get("value_scaled")?.as_i64()?;
    if revision.is_empty() || revision.len() > 128 || !(-MAX_SCALED..=MAX_SCALED).contains(&value) {
        return None;
    }
    Some((revision, value))
}

/// The coefficient and precision have already passed the published grant.
pub fn service_value(value: i64, decimal_places: u8) -> Value {
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.unsigned_abs();
    let literal = if decimal_places == 0 {
        value.to_string()
    } else {
        let divisor = 10_u64.pow(u32::from(decimal_places));
        format!(
            "{sign}{}.{:0width$}",
            absolute / divisor,
            absolute % divisor,
            width = usize::from(decimal_places)
        )
    };
    Value::Number(literal.parse::<Number>().expect("validated scaled decimal"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_decimals_support_exponents_normalization_and_coefficient_boundaries() {
        for (literal, coefficient, places) in [
            ("-1.25e-2", -125, 4),
            ("100e-8", 1, 6),
            ("1.0000000", 1, 0),
            ("1e+3", 1000, 0),
            ("-0", 0, 0),
            ("0.100000", 1, 1),
            ("1000000000000000", MAX_SCALED, 0),
            ("-1000000000.000001", -1_000_000_000_000_001, 6),
        ] {
            let expected = (coefficient.unsigned_abs() <= MAX_SCALED as u64).then_some(Decimal {
                coefficient,
                places,
            });
            assert_eq!(decimal(literal), expected, "{literal}");
        }
        for literal in [
            "0.0000001",
            "0.10000000000000001",
            "1000000000000001",
            "1e9999999999",
            "1e-2147483648",
            "NaN",
            "inf",
            "1.",
            ".1",
            "+1",
            "01",
            " 1",
            "1 ",
            "1e",
            "1e2e3",
        ] {
            assert!(decimal(literal).is_none(), "{literal}");
        }
        assert!(decimal(&format!("0.{}1", "0".repeat(128))).is_none());
    }

    #[test]
    fn exact_metadata_grid_and_service_values_never_round_through_floats() {
        let mut state: Value = serde_json::from_str(r#"{"entity_id":"number.a","state":"-0.2","attributes":{"min":-0.3,"max":1e0,"step":1e-1}}"#).unwrap();
        let observed = observe(Operation::SetNumber, "number.a", Some(&state)).unwrap();
        assert_eq!(
            (
                observed.constraints.min,
                observed.constraints.max,
                observed.constraints.step,
                observed.value
            ),
            (-3, 10, 1, -2)
        );
        assert_eq!(observed.constraints.decimal_places, 1);
        assert_eq!(service_value(-2, 1).to_string(), "-0.2");
        assert_eq!(service_value(1, 6).to_string(), "0.000001");
        assert_eq!(
            service_value(MAX_SCALED, 6).to_string(),
            "1000000000.000000"
        );
        state["attributes"]["step"] = serde_json::from_str("0.10000000000000001").unwrap();
        assert!(observe(Operation::SetNumber, "number.a", Some(&state)).is_none());
        state["attributes"]["step"] = json!(0.2);
        assert!(observe(Operation::SetNumber, "number.a", Some(&state)).is_none());
        state["state"] = json!("-0.1");
        let observation = observe(Operation::SetNumber, "number.a", Some(&state)).unwrap();
        assert!(observation.constraints.accepts(1));
        assert!(!observation.constraints.accepts(0));
    }

    #[test]
    fn malformed_and_unrepresentable_number_metadata_stays_read_only() {
        let valid =
            json!({"entity_id":"number.a","state":"1","attributes":{"min":0,"max":10,"step":1}});
        for (field, value) in [
            ("min", json!("0")),
            ("max", Value::Null),
            ("step", json!(true)),
            ("step", json!(0)),
            ("step", json!(-1)),
            ("min", json!(11)),
            ("unit_of_measurement", json!(17)),
            ("max", json!(MAX_SCALED)),
            ("step", json!(0.000001)),
        ] {
            let mut state = valid.clone();
            state["attributes"][field] = value;
            // The two last fields are individually supported; their combination
            // cannot fit the shared scaled-integer bound.
            if field == "max" || field == "step" && state["attributes"][field] == json!(0.000001) {
                if state["attributes"][field] == json!(MAX_SCALED) {
                    state["attributes"]["step"] = json!(0.1);
                }
                if state["attributes"][field] == json!(0.000001) {
                    state["attributes"]["max"] = json!(MAX_SCALED);
                }
            }
            assert!(
                observe(Operation::SetNumber, "number.a", Some(&state)).is_none(),
                "{field}: {state}"
            );
        }
        for state_value in [
            json!("unknown"),
            json!("unavailable"),
            json!("0.5"),
            json!("11"),
            json!(1),
            Value::Null,
        ] {
            let mut state = valid.clone();
            state["state"] = state_value;
            assert!(observe(Operation::SetNumber, "number.a", Some(&state)).is_none());
        }
    }

    #[test]
    fn cover_position_requires_integer_metadata_and_only_position_feature() {
        let valid = json!({"entity_id":"cover.a","state":"closed","attributes":{"current_position":0,"supported_features":4}});
        for state_name in ["open", "closed", "opening", "closing"] {
            let mut state = valid.clone();
            state["state"] = json!(state_name);
            let observed = observe(Operation::SetCoverPosition, "cover.a", Some(&state)).unwrap();
            assert_eq!(observed.constraints.unit.as_deref(), Some("%"));
            assert!(observed.constraints.accepts(100));
        }
        for (field, value) in [
            ("current_position", json!(1.0)),
            ("current_position", json!("1")),
            ("current_position", json!(-1)),
            ("current_position", json!(101)),
            ("current_position", Value::Null),
            ("supported_features", json!(11)),
            ("supported_features", json!(4.0)),
            ("supported_features", json!("4")),
            ("supported_features", json!(-1)),
        ] {
            let mut state = valid.clone();
            state["attributes"][field] = value;
            assert!(observe(Operation::SetCoverPosition, "cover.a", Some(&state)).is_none());
        }
        assert!(params(&json!({"input_revision":"r1","value_scaled":1.0})).is_none());
        assert!(
            params(&json!({"input_revision":"r1","value_scaled":1,"entity_id":"cover.a"}))
                .is_none()
        );
    }
}
