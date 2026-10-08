//! Stateless presentation budgets over complete, already-redacted JSON reports.
use crate::{cli::Cli, evidence::digest};
use serde_json::{Value, json};

#[derive(Clone)]
pub(crate) struct Policy {
    chars: Option<usize>,
    bytes: Option<usize>,
    items: Option<usize>,
    cursor: Option<String>,
    complete: bool,
}
pub(crate) struct ResultDocument {
    pub document: String,
    pub error: Option<&'static str>,
}

#[derive(Clone, Copy)]
enum Kind {
    Array,
    Map,
    String,
}
struct Collection {
    path: String,
    kind: Kind,
    items: Vec<Value>,
}
impl Collection {
    fn add(&self, report: &mut Value, index: usize) {
        let target = report.pointer_mut(&self.path).unwrap();
        match self.kind {
            Kind::Array => target
                .as_array_mut()
                .unwrap()
                .push(self.items[index].clone()),
            Kind::Map => target
                .as_object_mut()
                .unwrap()
                .extend(self.items[index].as_object().unwrap().clone()),
            Kind::String => *target = self.items[index].clone(),
        }
    }
    fn increment(&self, index: usize, first: bool) -> (usize, usize) {
        let text = self.items[index].to_string();
        let mut bytes = text.len();
        let mut chars = text.chars().count();
        if matches!(self.kind, Kind::Map | Kind::String) {
            bytes -= 2;
            chars -= 2;
        }
        if !matches!(self.kind, Kind::String) && !first {
            bytes += 1;
            chars += 1;
        }
        (bytes, chars)
    }
}

// Only generated collections are pageable. Payload arrays and measurement boundaries
// remain atomic; a huge record must be retrieved with a larger budget or complete mode.
fn collections(report: &mut Value) -> Vec<Collection> {
    let mut paths: Vec<String> = [
        "/ambiguous_groups",
        "/unmatched_events",
        "/orphans",
        "/stats",
        "/operations",
        "/threshold_violations",
        "/errors/clusters",
        "/errors/summary/longest_blocking/pattern",
        "/event_timeline/ambiguous_groups",
        "/event_timeline/events",
        "/event_timeline/intervals",
        "/event_timeline/incomplete",
        "/trace/event_timeline/ambiguous_groups",
        "/trace/event_timeline/events",
        "/trace/event_timeline/intervals",
        "/trace/event_timeline/incomplete",
        "/comparisons",
        "/unique_to_log1",
        "/unique_to_log2",
        "/search/groups",
        "/search/entries",
        "/extract/groups",
        "/extract/rows",
        "/extract/rejected_expansions",
        "/trace/entries",
        "/logs",
        "/info/components",
        "/metadata/components",
        "/metadata/levels",
        "/metadata/entry_types",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if let Some(files) = report.pointer("/coverage/files").and_then(Value::as_array) {
        paths.extend(
            (0..files.len()).map(|i| format!("/coverage/files/{i}/normalization_diagnostics")),
        );
    }
    if let Some(inputs) = report
        .pointer("/report_metadata/evidence/inputs")
        .and_then(Value::as_array)
    {
        paths.extend((0..inputs.len()).map(|i| {
            format!("/report_metadata/evidence/inputs/{i}/coverage/normalization_diagnostics")
        }));
    }
    paths.push("/evidence_records".into());
    let mut result = Vec::new();
    for path in paths {
        let Some(value) = report.pointer_mut(&path) else {
            continue;
        };
        let (kind, items) = match value {
            Value::Array(items) => (Kind::Array, std::mem::take(items)),
            Value::Object(map) => (
                Kind::Map,
                std::mem::take(map)
                    .into_iter()
                    .map(|(key, value)| json!({key:value}))
                    .collect(),
            ),
            Value::String(text) if !text.is_empty() => {
                (Kind::String, vec![Value::String(std::mem::take(text))])
            }
            _ => continue,
        };
        result.push(Collection { path, kind, items });
    }
    result
}

impl Policy {
    pub fn from_cli(cli: &Cli) -> Self {
        Self {
            chars: cli.report_max_chars,
            bytes: cli.report_max_bytes,
            items: if cli.complete_output {
                None
            } else {
                Some(cli.report_max_items.unwrap_or(100))
            },
            cursor: cli.report_cursor.clone(),
            complete: cli.complete_output,
        }
    }
    fn fits(&self, bytes: usize, chars: usize) -> bool {
        self.bytes.is_none_or(|limit| bytes <= limit)
            && self.chars.is_none_or(|limit| chars <= limit)
    }
    pub fn apply(&self, mut report: Value) -> ResultDocument {
        if let Some(omissions) = report
            .pointer_mut("/report_metadata/evidence/omissions")
            .and_then(Value::as_object_mut)
        {
            omissions.insert(
                "basis".into(),
                json!("legacy_view_before_common_pagination"),
            );
            omissions.insert("pagination".into(), json!("see_retrieval_collections"));
        }
        let report_digest = digest(report.to_string().as_bytes());
        let binding = digest(
            json!([
                1,
                report_digest,
                report.pointer("/report_metadata/evidence/snapshot_id"),
                report.pointer("/report_metadata/evidence/profile_sha256"),
                report.pointer("/report_metadata/evidence/query_sha256"),
                report.pointer("/report_metadata/evidence/redaction")
            ])
            .to_string()
            .as_bytes(),
        );
        let groups = collections(&mut report);
        let total: usize = groups.iter().map(|group| group.items.len()).sum();
        let parse_cursor = |cursor: &str| -> Option<usize> {
            let (id, offset) = cursor.split_once(':')?;
            if id != binding || offset.is_empty() || !offset.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let offset: usize = offset.parse().ok()?;
            (offset <= total).then_some(offset)
        };
        let offset = match self.cursor.as_deref().map(parse_cursor) {
            Some(Some(offset)) => offset,
            Some(None) => {
                report["retrieval"] = json!({"version":1,"status":"invalid_cursor","reason":"input_profile_query_redaction_or_report_changed_or_cursor_malformed","total_items":total,"next_cursor":null,"report_sha256":report_digest});
                report["retrieval"]["metadata_over_budget"] = json!(false);
                report["retrieval"]["retrieve"] = json!(
                    "Restart the same command without --report-cursor; previous references require revalidation."
                );
                let document = format!("{report}\n");
                if !self.fits(document.len(), document.chars().count()) {
                    report["retrieval"]["metadata_over_budget"] = json!(true);
                }
                return ResultDocument {
                    document: format!("{report}\n"),
                    error: Some(
                        "Report cursor is invalid or input/profile/query/redaction/report changed; restart retrieval",
                    ),
                };
            }
            None => 0,
        };
        let token = |offset| format!("{binding}:{offset}");
        let mut prior = Vec::new();
        let mut remaining_offset = offset;
        for group in &groups {
            let consumed = remaining_offset.min(group.items.len());
            remaining_offset -= consumed;
            prior.push(consumed);
        }
        let mut shown = vec![0usize; groups.len()];
        let metadata = |status: &str, shown: &[usize], next: Option<String>| {
            json!({"version":1,"status":status,"metadata_over_budget":false,"mode":if self.complete{"complete"}else{"page"},
                "units":{"bytes":"serialized_UTF8_including_newline","characters":"serialized_Unicode_scalars_including_newline","items":"collection_pointer_and_original_ordinal"},
                "budgets":{"bytes":self.bytes,"characters":self.chars,"items":self.items},
                "total_items":total,"prior_items":offset,"displayed_items":shown.iter().sum::<usize>(),
                "remaining_items":total-offset-shown.iter().sum::<usize>(),"next_cursor":next,"report_sha256":report_digest,
                "source_records":{"path":"/evidence_records","ordering":"timestamp_then_input_ordinal_then_physical_record_order","selection":"global_filter_and_trace_selector"},
                "collections":groups.iter().enumerate().map(|(i,group)|json!({"path":group.path,"total":group.items.len(),"prior":prior[i],"displayed":shown[i],"remaining":group.items.len()-prior[i]-shown[i]})).collect::<Vec<_>>(),
                "retrieve":"Rerun the same command/profile/inputs/redaction with --report-cursor NEXT_CURSOR; budgets may increase. Use --complete-output without budgets/cursor for all collections. A blocked item does not advance the cursor."})
        };
        // Measure the skeleton once; each packing attempt serializes only the small
        // retrieval metadata and the candidate item, never the entire growing report.
        report["retrieval"] = Value::Null;
        let skeleton = format!("{report}\n");
        let base_bytes = skeleton.len() - 4;
        let base_chars = skeleton.chars().count() - 4;
        let mut detail_bytes = 0usize;
        let mut detail_chars = 0usize;
        let mut displayed = 0;
        let mut blocked = false;
        let initial = metadata(
            if offset == total { "complete" } else { "page" },
            &shown,
            (offset < total).then(|| token(offset)),
        )
        .to_string();
        let mandatory_over = !self.fits(
            base_bytes + initial.len(),
            base_chars + initial.chars().count(),
        );
        if !mandatory_over || self.complete {
            'packing: for (i, group) in groups.iter().enumerate() {
                for index in prior[i]..group.items.len() {
                    if self.items.is_some_and(|limit| displayed >= limit) {
                        break 'packing;
                    }
                    let (bytes, chars) = group.increment(index, shown[i] == 0);
                    shown[i] += 1;
                    let next = offset + displayed + 1;
                    let candidate = metadata(
                        if next == total { "complete" } else { "page" },
                        &shown,
                        (next < total).then(|| token(next)),
                    )
                    .to_string();
                    if !self.fits(
                        base_bytes
                            .saturating_add(detail_bytes)
                            .saturating_add(bytes)
                            .saturating_add(candidate.len()),
                        base_chars
                            .saturating_add(detail_chars)
                            .saturating_add(chars)
                            .saturating_add(candidate.chars().count()),
                    ) {
                        shown[i] -= 1;
                        blocked = displayed == 0;
                        break 'packing;
                    }
                    group.add(&mut report, index);
                    displayed += 1;
                    detail_bytes += bytes;
                    detail_chars += chars;
                }
            }
        }
        let next_offset = offset + displayed;
        let status = if self.complete {
            "complete"
        } else if mandatory_over {
            "mandatory_metadata_over_budget"
        } else if blocked {
            "oversized_item"
        } else if self.items == Some(0) && offset < total {
            "item_limit_zero"
        } else if next_offset == total {
            "complete"
        } else {
            "page"
        };
        report["retrieval"] = metadata(
            status,
            &shown,
            (next_offset < total).then(|| token(next_offset)),
        );
        if mandatory_over {
            report["retrieval"]["metadata_over_budget"] = json!(true);
        }
        let mut document = format!("{report}\n");
        if !self.fits(document.len(), document.chars().count()) && displayed == 0 && !self.complete
        {
            report["retrieval"]["status"] = json!("mandatory_metadata_over_budget");
            report["retrieval"]["metadata_over_budget"] = json!(true);
            document = format!("{report}\n");
        }
        debug_assert!(displayed == 0 || self.fits(document.len(), document.chars().count()));
        ResultDocument {
            document,
            error: None,
        }
    }
}
