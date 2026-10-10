//! Explicitly lossy presentation for commands without a paginated report contract.
use serde_json::{Value, json};

pub(crate) fn project(report: &Value) -> Value {
    let mut omissions = Vec::new();
    let displayed = bound(report, "", &mut omissions);
    let next_step = if report.get("profile_mappings").is_some() {
        "Use profile mappings inspect without --summary for complete registry metadata; do not repeat a mutation to expand its report."
    } else {
        "Omit --summary to retrieve the complete report."
    };
    json!({
        "summary_version":1,
        "report":displayed,
        "omissions":omissions,
        "limitations":["This is a presentation summary, not a complete evidence inventory. Scalars, provenance, coverage and diagnostics are retained; no byte limit is implied."],
        "next_steps":[next_step]
    })
}

fn bound(value: &Value, path: &str, omissions: &mut Vec<Value>) -> Value {
    match value {
        Value::Array(items) => {
            if items.len() > 5 {
                omissions.push(
                    json!({"path":path,"total":items.len(),"displayed":5,"omitted":items.len()-5}),
                );
            }
            Value::Array(
                items
                    .iter()
                    .take(5)
                    .enumerate()
                    .map(|(index, item)| bound(item, &format!("{path}/{index}"), omissions))
                    .collect(),
            )
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let preserve = matches!(
                        key.as_str(),
                        "report_metadata"
                            | "provenance"
                            | "coverage"
                            | "inputs"
                            | "diagnostics"
                            | "limitations"
                            | "assumptions"
                            | "warnings"
                            | "errors"
                            | "suitability"
                    );
                    let pointer = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                    (
                        key.clone(),
                        if preserve {
                            value.clone()
                        } else {
                            bound(value, &pointer, omissions)
                        },
                    )
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_disclose_omissions_and_preserve_unknowns_and_coverage() {
        let coverage = json!({"inputs":[1,2,3,4,5,6],"unparsed":1});
        let result = project(
            &json!({"a/b~c":[null,1,2,3,4,5],"coverage":coverage,"diagnostics":[1,2,3,4,5,6],"status":"unsupported"}),
        );
        assert_eq!(result["report"]["a/b~c"][0], Value::Null);
        assert_eq!(result["report"]["coverage"], coverage);
        assert_eq!(result["report"]["diagnostics"].as_array().unwrap().len(), 6);
        assert_eq!(result["report"]["status"], "unsupported");
        assert_eq!(
            result["omissions"],
            json!([{"path":"/a~1b~0c","total":6,"displayed":5,"omitted":1}])
        );
    }
}
