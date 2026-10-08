use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NormalizationRules {
    pub root_path: String,
    pub expand_rows: bool,
    pub decode_paths: Vec<String>,
    pub row_decode_paths: Vec<String>,
    pub fields: BTreeMap<String, String>,
    pub timestamp_unit: Option<TimestampUnit>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampUnit {
    Seconds,
    Milliseconds,
    Microseconds,
    Nanoseconds,
}

#[derive(Debug, Clone, Serialize)]
pub struct RowDiagnostic {
    pub line: usize,
    pub row_path: String,
    pub field: String,
    pub reason: String,
}

pub fn normalize(
    text: &str,
    line: usize,
    rules: &NormalizationRules,
) -> Vec<(String, Result<Value, RowDiagnostic>)> {
    let failure = |row_path: &str, field: &str, reason: &str| RowDiagnostic {
        line,
        row_path: row_path.into(),
        field: field.into(),
        reason: reason.into(),
    };
    let mut root: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return vec![(String::new(), Err(failure("", "", "invalid_json")))],
    };
    for path in &rules.decode_paths {
        let Some(value) = root.pointer_mut(path) else {
            return vec![(
                path.clone(),
                Err(failure(path, path, "missing_decode_path")),
            )];
        };
        let Some(encoded) = value.as_str() else {
            return vec![(
                path.clone(),
                Err(failure(path, path, "decode_requires_string")),
            )];
        };
        let decoded = match serde_json::from_str(encoded) {
            Ok(v) => v,
            Err(_) => {
                return vec![(
                    path.clone(),
                    Err(failure(path, path, "invalid_json_string")),
                )];
            }
        };
        *value = decoded;
    }
    let Some(selected) = root.pointer(&rules.root_path) else {
        return vec![(
            rules.root_path.clone(),
            Err(failure(&rules.root_path, "", "missing_root_path")),
        )];
    };
    let rows: Vec<(String, &Value)> = if rules.expand_rows {
        match selected.as_array() {
            Some(rows) if !rows.is_empty() => rows
                .iter()
                .enumerate()
                .map(|(i, row)| (format!("{}/{}", rules.root_path, i), row))
                .collect(),
            Some(_) => {
                return vec![(
                    rules.root_path.clone(),
                    Err(failure(&rules.root_path, "", "empty_expansion")),
                )];
            }
            None => {
                return vec![(
                    rules.root_path.clone(),
                    Err(failure(&rules.root_path, "", "expansion_requires_array")),
                )];
            }
        }
    } else {
        vec![(rules.root_path.clone(), selected)]
    };
    rows.into_iter()
        .map(|(path, row)| {
            let result = (|| {
                let mut row = row.clone();
                for pointer in &rules.row_decode_paths {
                    let encoded = row
                        .pointer(pointer)
                        .and_then(Value::as_str)
                        .ok_or_else(|| failure(&path, pointer, "row_decode_requires_string"))?;
                    let decoded = serde_json::from_str(encoded)
                        .map_err(|_| failure(&path, pointer, "invalid_json_string"))?;
                    *row.pointer_mut(pointer).expect("validated pointer") = decoded;
                }
                let mut mapped = if rules.fields.is_empty() {
                    row.as_object()
                        .cloned()
                        .ok_or_else(|| failure(&path, "", "row_requires_object_or_field_mapping"))?
                } else {
                    serde_json::Map::new()
                };
                for (field, pointer) in &rules.fields {
                    let value = row
                        .pointer(pointer)
                        .ok_or_else(|| failure(&path, field, "missing_field"))?;
                    if value.is_null() {
                        return Err(failure(&path, field, "null_field"));
                    }
                    if matches!(
                        field.as_str(),
                        "timestamp" | "level" | "component" | "component_id" | "message"
                    ) && !value.is_string()
                        && !(field == "timestamp"
                            && rules.timestamp_unit.is_some()
                            && value.is_i64())
                    {
                        return Err(failure(&path, field, "wrong_field_type"));
                    }
                    mapped.insert(field.clone(), value.clone());
                }
                let timestamp = mapped
                    .get("timestamp")
                    .ok_or_else(|| failure(&path, "timestamp", "missing_field"))?;
                if let Some(unit) = rules.timestamp_unit {
                    let n = timestamp
                        .as_i64()
                        .ok_or_else(|| failure(&path, "timestamp", "timestamp_requires_integer"))?;
                    let scale = match unit {
                        TimestampUnit::Seconds => 1,
                        TimestampUnit::Milliseconds => 1000,
                        TimestampUnit::Microseconds => 1_000_000,
                        TimestampUnit::Nanoseconds => 1_000_000_000,
                    };
                    let date = chrono::DateTime::from_timestamp(
                        n.div_euclid(scale),
                        (n.rem_euclid(scale) * (1_000_000_000 / scale)) as u32,
                    )
                    .ok_or_else(|| failure(&path, "timestamp", "timestamp_out_of_range"))?;
                    mapped.insert(
                        "timestamp".into(),
                        json!(date.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)),
                    );
                } else if !timestamp.is_string() {
                    return Err(failure(
                        &path,
                        "timestamp",
                        "timestamp_requires_string_or_explicit_unit",
                    ));
                }
                Ok(Value::Object(mapped))
            })();
            (path, result)
        })
        .collect()
}

pub fn schema_preview(file: &std::path::Path, samples: usize) -> Result<Value, std::io::Error> {
    use std::io::{BufRead, BufReader};
    fn fields(value: &Value, path: String, out: &mut BTreeMap<String, String>, depth: usize) {
        let kind = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        out.insert(path.clone(), kind.into());
        if depth >= 4 {
            return;
        }
        match value {
            Value::Object(map) => {
                for (key, value) in map.iter().take(20) {
                    fields(
                        value,
                        format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                        out,
                        depth + 1,
                    );
                }
            }
            Value::Array(array) => {
                for (i, value) in array.iter().take(3).enumerate() {
                    fields(value, format!("{path}/{i}"), out, depth + 1);
                }
            }
            _ => {}
        }
    }
    let mut rows = Vec::new();
    for (i, line) in BufReader::new(std::fs::File::open(file)?)
        .lines()
        .enumerate()
    {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row = match serde_json::from_str::<Value>(&line) {
            Ok(value) => {
                let mut paths = BTreeMap::new();
                fields(&value, String::new(), &mut paths, 0);
                json!({"line":i+1,"paths":paths})
            }
            Err(_) => json!({"line":i+1,"error":"invalid_json"}),
        };
        rows.push(row);
        if rows.len() >= samples {
            break;
        }
    }
    Ok(
        json!({"schema_preview":{"file":file,"samples":rows,"max_depth":4,"max_fields_per_object":20,"max_items_per_array":3,"json_strings_decoded":false}}),
    )
}
