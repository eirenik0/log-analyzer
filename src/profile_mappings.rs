//! Explicit, minimal source/profile metadata; lookup never creates files.
use crate::{
    cli::{Cli, Commands, MappingAction, MappingScope, OperationType},
    config, evidence,
    profile_resolution::{self, ProfileChoice, ResolutionRequest, ValidatedSelection},
    profile_validation::Purpose,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct SourceKey {
    project_context: Option<String>,
    sources: Vec<String>,
    kind: String,
    purpose: Purpose,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    expected_sha256: String,
    basis: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: String,
    key: SourceKey,
    profile: ProfileChoice,
    selected_parsers: Vec<config::LogFormat>,
    event_contract: u32,
    structural_contract: u32,
    resolution_contract: u32,
    provenance: Provenance,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    version: u32,
    scope: MappingScope,
    entries: Vec<Entry>,
}

pub(crate) struct MappingCandidate {
    pub origin: String,
    pub choice: Value,
    pub config: std::result::Result<config::AnalyzerConfig, String>,
}
pub(crate) struct Lookup {
    pub candidates: Vec<MappingCandidate>,
    pub diagnostics: Value,
    pub ambiguous: Option<String>,
}
struct Store {
    root: PathBuf,
    path: PathBuf,
    scope: MappingScope,
}
fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
fn root(path: Option<&Path>) -> Result<PathBuf> {
    Ok(fs::canonicalize(
        path.map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?),
    )?)
}
fn default_user_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|p| p.join(".config/log-analyzer/profile-mappings.json"))
}
impl Store {
    fn new(scope: MappingScope, root: PathBuf, path: Option<&Path>) -> Result<Self> {
        let path = match path {
            Some(p) => p.to_owned(),
            None => match scope {
                MappingScope::Project => root.join(".log-analyzer/profile-mappings.json"),
                MappingScope::User => {
                    default_user_path().ok_or("User home unavailable; supply --registry")?
                }
            },
        };
        Ok(Self { root, path, scope })
    }
    fn read(&self) -> Result<Registry> {
        let bytes = match fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Registry {
                    version: 1,
                    scope: self.scope,
                    entries: vec![],
                });
            }
            Err(e) => return Err(e.into()),
        };
        let registry: Registry = serde_json::from_slice(&bytes)?;
        if registry.version != 1 || registry.scope != self.scope {
            return Err("Registry version/scope mismatch".into());
        }
        for entry in &registry.entries {
            if entry.id != digest(&entry.key)?
                || entry.key.sources.is_empty()
                || entry.key.sources.len() != entry.selected_parsers.len()
                || entry.profile.preset.is_some() == entry.profile.config.is_some()
                || !["request", "event", "command"].contains(&entry.key.kind.as_str())
                || entry.provenance.basis != "complete_current_sample_independent_assertions"
                || !valid_digest(&entry.provenance.expected_sha256)
                || !valid_digest(&entry.profile.sha256)
            {
                return Err("Malformed mapping entry; inspect and repair the registry without overwriting it".into());
            }
            if self.scope == MappingScope::Project
                && (entry.key.project_context.is_some()
                    || entry
                        .key
                        .sources
                        .iter()
                        .any(|p| !safe_relative(Path::new(p)))
                    || entry
                        .profile
                        .config
                        .as_ref()
                        .is_some_and(|p| !safe_relative(p)))
            {
                return Err("Project mapping paths must stay inside the project root".into());
            }
            if self.scope == MappingScope::User
                && (entry
                    .key
                    .project_context
                    .as_ref()
                    .is_none_or(|p| !Path::new(p).is_absolute())
                    || entry
                        .key
                        .sources
                        .iter()
                        .any(|p| !Path::new(p).is_absolute())
                    || entry
                        .profile
                        .config
                        .as_ref()
                        .is_some_and(|p| !p.is_absolute()))
            {
                return Err("User mapping paths require absolute project context".into());
            }
        }
        Ok(registry)
    }
    fn key(&self, files: &[PathBuf], kind: OperationType, purpose: Purpose) -> Result<SourceKey> {
        let sources = files
            .iter()
            .map(|p| {
                fs::canonicalize(p)
                    .map_err(Into::into)
                    .and_then(|p| self.label(&p))
            })
            .collect::<Result<Vec<_>>>()?;
        let kind = match kind {
            OperationType::Request => "request",
            OperationType::Event => "event",
            OperationType::Command => "command",
        }
        .into();
        Ok(SourceKey {
            project_context: (self.scope == MappingScope::User)
                .then(|| {
                    self.root
                        .to_str()
                        .ok_or("Persistent project context must be UTF-8")
                        .map(str::to_owned)
                })
                .transpose()?,
            sources,
            kind,
            purpose,
        })
    }
    fn label(&self, path: &Path) -> Result<String> {
        let label = match self.scope {MappingScope::User=>path,MappingScope::Project=>path.strip_prefix(&self.root).map_err(|_|"Source/profile escapes project root; use user scope or choose a containing --project-root")?};
        Ok(label
            .to_str()
            .ok_or("Persistent mapping paths must be UTF-8")?
            .to_owned())
    }
    fn entry(&self, key: SourceKey, selected: ValidatedSelection) -> Result<Entry> {
        let mut profile = selected.profile;
        if let Some(path) = profile.config {
            profile.config = Some(PathBuf::from(self.label(&fs::canonicalize(path)?)?));
        }
        Ok(Entry {
            id: digest(&key)?,
            key,
            profile,
            selected_parsers: selected
                .sources
                .into_iter()
                .map(|s| s.selected_parser)
                .collect(),
            event_contract: 2,
            structural_contract: 1,
            resolution_contract: 2,
            provenance: Provenance {
                expected_sha256: selected.expected_sha256,
                basis: "complete_current_sample_independent_assertions".into(),
            },
        })
    }
    fn mutate(&self, replacement: Option<Entry>, prior: Option<(&str, &str)>) -> Result<Committed> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        reject_symlink(&self.path)?;
        let resolved_path = fs::canonicalize(parent)?
            .join(self.path.file_name().ok_or("Registry needs a filename")?);
        if resolved_path != self.path {
            return Store {
                root: self.root.clone(),
                path: resolved_path,
                scope: self.scope,
            }
            .mutate(replacement, prior);
        }
        let lock_path = self.path.with_file_name(format!(
            "{}.lock",
            self.path
                .file_name()
                .ok_or("Registry needs a filename")?
                .to_string_lossy()
        ));
        reject_symlink(&lock_path)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    return Err("Registry busy; retry the mutation".into());
                }
                Err(e) => return Err(e.into()),
            }
        }
        reject_symlink(&self.path)?;
        let mut registry = self.read()?;
        if let Some((id, expected)) = prior {
            let matches: Vec<_> = registry
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.id == id)
                .collect();
            if matches.len() != 1 || digest(matches[0].1)? != expected {
                return Err(
                    "Registry conflict: entry changed, missing, or ambiguous; inspect again".into(),
                );
            }
            registry.entries.remove(matches[0].0);
        }
        if let Some(entry) = replacement {
            if registry.entries.iter().any(|e| e.key == entry.key) {
                return Err(
                    "Source key already remembered; inspect then replace with its entry digest"
                        .into(),
                );
            }
            registry.entries.push(entry);
        }
        registry.entries.sort_by(|a, b| a.id.cmp(&b.id));
        let report = self.view(&registry)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&serde_json::to_vec_pretty(&registry)?)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        persist_registry(temporary, &self.path)?;
        drop(lock);
        Ok(Committed { report })
    }
    fn view(&self, registry: &Registry) -> Result<Value> {
        let entries = registry
            .entries
            .iter()
            .map(|entry| Ok(json!({"entry":entry,"digest":digest(entry)?})))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"profile_mappings":{"version":1,"scope":self.scope,"registry":evidence::path_label(&self.path),"project_root":evidence::path_label(&self.root),"entries":entries,"raw_evidence_persisted":false}}),
        )
    }
}
struct Committed {
    report: Value,
}
pub(crate) struct StagedReport {
    path: PathBuf,
    temporary: tempfile::NamedTempFile,
}
impl StagedReport {
    pub(crate) fn prepare(path: &Path) -> Result<Self> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.permissions().readonly()
                {
                    return Err(
                        "Report destination must be a writable regular file or a new file".into(),
                    );
                }
                OpenOptions::new().write(true).open(path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Ok(Self {
            path: path.to_owned(),
            temporary: tempfile::NamedTempFile::new_in(parent)?,
        })
    }
    pub(crate) fn save_compact(mut self, report: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.temporary, report)?;
        self.temporary.write_all(b"\n")?;
        self.temporary.as_file().sync_all()?;
        persist_registry(self.temporary, &self.path)?;
        Ok(())
    }
    pub(crate) fn save(mut self, report: &Value) -> Result<()> {
        let rendered = serde_json::to_string_pretty(report)?;
        self.temporary
            .write_all(crate::output::format_report(&rendered).as_bytes())?;
        self.temporary.as_file().sync_all()?;
        persist_registry(self.temporary, &self.path)?;
        Ok(())
    }
}
fn deliver_report(
    mut report: Value,
    committed: bool,
    staged: Option<StagedReport>,
) -> Result<Value> {
    report["profile_mappings"]["mutation"] =
        json!({"status":if committed {"committed"} else {"not_requested"}});
    report["profile_mappings"]["report_save"] =
        json!({"status":if staged.is_some() {"succeeded"} else {"not_requested"}});
    if let Some(staged) = staged
        && let Err(error) = staged.save(&report)
    {
        if !committed {
            return Err(error);
        }
        report["profile_mappings"]["report_save"]["status"] = json!("failed");
        eprintln!(
            "Warning: registry mutation committed, but report saving failed. Do not retry the mutation; rerun profile-mappings inspect with a separate --output path."
        );
    }
    Ok(report)
}
#[cfg(not(windows))]
fn persist_registry(temporary: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
    temporary.persist(path).map(|_| ()).map_err(|e| e.error)
}
#[cfg(windows)]
fn persist_registry(mut temporary: tempfile::NamedTempFile, path: &Path) -> std::io::Result<()> {
    // Windows can deny replacement while a reader has the old file open.
    // Retain the synced temporary file and writer lock; never remove the old file.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match temporary.persist(path) {
            Ok(_) => return Ok(()),
            Err(error)
                if matches!(error.error.raw_os_error(), Some(5 | 32 | 33))
                    && Instant::now() < deadline =>
            {
                temporary = error.file;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.error),
        }
    }
}
// Resolve existing ancestors without creating a destination; later nonexistent
// components remain normalized so inspection cannot overwrite its own store.
pub(crate) fn destination_identity(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(_) => {
                resolved.push(component.as_os_str());
                continue;
            }
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            _ => resolved.push(component.as_os_str()),
        }
        match fs::symlink_metadata(&resolved) {
            Ok(_) => {
                resolved = fs::canonicalize(&resolved)
                    .map_err(|_| "Ambiguous report destination; use a separate resolvable path")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(resolved)
}
pub(crate) fn same_destination(a: &Path, b: &Path, compare_contents: bool) -> bool {
    #[cfg(unix)]
    let _ = compare_contents;
    if a == b {
        return true;
    }
    // Conservatively reject case-only aliases even on case-sensitive mounts.
    if a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase() {
        return true;
    }
    if let (Ok(a_meta), Ok(b_meta)) = (fs::metadata(a), fs::metadata(b)) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if a_meta.dev() == b_meta.dev() && a_meta.ino() == b_meta.ino() {
                return true;
            }
        }
        // Conservatively reject a copy indistinguishable from the current store
        // when native file identity is unavailable.
        #[cfg(not(unix))]
        if compare_contents
            && a_meta.len() == b_meta.len()
            && a_meta.is_file()
            && b_meta.is_file()
            && fs::read(a)
                .ok()
                .zip(fs::read(b).ok())
                .is_some_and(|(a, b)| a == b)
        {
            return true;
        }
    }
    false
}
fn protect_output(store: &Store, output: Option<&Path>) -> Result<()> {
    let Some(output) = output else {
        return Ok(());
    };
    let output = destination_identity(output)?;
    let registry = destination_identity(&store.path)?;
    let lock = registry.with_file_name(format!(
        "{}.lock",
        registry
            .file_name()
            .ok_or("Registry needs a filename")?
            .to_string_lossy()
    ));
    if same_destination(&output, &registry, true) || same_destination(&output, &lock, false) {
        return Err("Output destination conflicts with the registry or its lock; choose a separate report path".into());
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(
            "Registry/lock symlinks are unsupported for mutation; supply the actual registry path"
                .into(),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn valid_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn safe_relative(p: &Path) -> bool {
    !p.as_os_str().is_empty()
        && p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

pub(crate) fn lookup(
    project_root: Option<&Path>,
    project_path: Option<&Path>,
    user_path: Option<&Path>,
    files: &[PathBuf],
    kind: OperationType,
    purpose: Purpose,
) -> Result<Lookup> {
    let root = root(project_root)?;
    let mut candidates = vec![];
    let mut diagnostics = vec![];
    let mut ambiguous = None;
    for (scope, path, origin) in [
        (MappingScope::Project, project_path, "project_mapping"),
        (MappingScope::User, user_path, "user_mapping"),
    ] {
        if scope == MappingScope::User && path.is_none() && default_user_path().is_none() {
            diagnostics.push(
                json!({"origin":origin,"status":"unavailable","reason":"user_home_unavailable"}),
            );
            continue;
        }
        let store = Store::new(scope, root.clone(), path)?;
        let (registry, key) = match store
            .read()
            .and_then(|r| Ok((r, store.key(files, kind, purpose)?)))
        {
            Ok(v) => v,
            Err(error) => {
                diagnostics.push(json!({"origin":origin,"status":"invalid","reason":crate::output::source_path(&error.to_string())}));
                continue;
            }
        };
        let matching: Vec<_> = registry.entries.iter().filter(|e| e.key == key).collect();
        if matching.len() > 1 {
            diagnostics.push(json!({"origin":origin,"status":"ambiguous","reason":"multiple_exact_source_matches"}));
            if ambiguous.is_none() {
                ambiguous = Some(origin.into());
            }
            continue;
        }
        let Some(entry) = matching.first() else {
            diagnostics.push(json!({"origin":origin,"status":"no_match"}));
            continue;
        };
        if entry.event_contract != 2
            || entry.structural_contract != 1
            || entry.resolution_contract != 2
        {
            diagnostics.push(
                json!({"origin":origin,"status":"invalid","reason":"mapping_contract_changed"}),
            );
            continue;
        }
        let config_path = entry.profile.config.as_ref().map(|p| {
            if scope == MappingScope::Project {
                root.join(p)
            } else {
                p.clone()
            }
        });
        let bounded_profile = if scope == MappingScope::Project {
            config_path
                .as_ref()
                .map(|p| {
                    fs::canonicalize(p)
                        .map_err(|e| e.to_string())
                        .and_then(|actual| {
                            actual
                                .strip_prefix(&root)
                                .map(|_| ())
                                .map_err(|_| "mapping_profile_escapes_project_root".into())
                        })
                })
                .unwrap_or(Ok(()))
        } else {
            Ok(())
        };
        let config = bounded_profile
            .and_then(|()| {
                config::load_config(config_path.as_deref(), entry.profile.preset.as_deref())
                    .map_err(|e| e.to_string())
            })
            .and_then(|c| {
                if evidence::profile_digest(&c).map_err(|e| e.to_string())? == entry.profile.sha256
                {
                    Ok(c)
                } else {
                    Err("mapping_profile_digest_changed".into())
                }
            });
        let shapes:Vec<_>=files.iter().zip(&entry.selected_parsers).map(|(file,parser)|json!({"file":evidence::path_label(file),"selected_parser":parser})).collect();
        let choice = json!({"selector":{"config":config_path.as_deref().map(evidence::path_label),"preset":entry.profile.preset},"expected_shapes":shapes,"mapping_id":entry.id,"mapping_digest":digest(entry)?});
        diagnostics.push(json!({"origin":origin,"status":"requires_current_input_validation","mapping_id":entry.id}));
        candidates.push(MappingCandidate {
            origin: origin.into(),
            choice,
            config,
        });
    }
    Ok(Lookup {
        candidates,
        diagnostics: json!(diagnostics),
        ambiguous,
    })
}

pub(crate) fn run(cli: &Cli) -> Result<()> {
    let Commands::ProfileMappings {
        scope,
        project_root,
        registry,
        action,
    } = &cli.command
    else {
        unreachable!()
    };
    let store = Store::new(*scope, root(project_root.as_deref())?, registry.as_deref())?;
    protect_output(&store, cli.output.as_deref())?;
    let staged = cli
        .output
        .as_deref()
        .map(StagedReport::prepare)
        .transpose()?;
    let committed = !matches!(action, MappingAction::Inspect);
    let report = match action {
        MappingAction::Inspect => store.view(&store.read()?)?,
        MappingAction::Forget {
            entry_id,
            if_digest,
        } => store.mutate(None, Some((entry_id, if_digest)))?.report,
        MappingAction::Remember {
            files,
            kind,
            purpose,
            expected,
            candidate_config,
        }
        | MappingAction::Replace {
            files,
            kind,
            purpose,
            expected,
            candidate_config,
            ..
        } => {
            // No registry lock during parsing, hashing or semantic validation.
            let key = store.key(files, *kind, *purpose)?;
            let result = profile_resolution::resolve(ResolutionRequest {
                cli,
                files,
                kind: *kind,
                purpose: *purpose,
                expected: Some(expected),
                candidate_config,
                association: None,
                mappings: None,
            })?;
            let selected=result.validated_selection.ok_or("Nothing eligible to remember: supply complete independent assertions and a structurally compatible selected profile")?;
            let entry = store.entry(key, selected)?;
            let prior = match action {
                MappingAction::Replace {
                    entry_id,
                    if_digest,
                    ..
                } => Some((entry_id.as_str(), if_digest.as_str())),
                _ => None,
            };
            store.mutate(Some(entry), prior)?.report
        }
    };
    crate::output::clear_evidence();
    crate::output::set_metadata(crate::build_info::metadata("profile-mappings"), false);
    let report = deliver_report(report, committed, staged)?;
    let rendered = serde_json::to_string_pretty(&report)?;
    crate::output::print(format_args!("{rendered}\n"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    fn entry(name: &str) -> Entry {
        let key = SourceKey {
            project_context: None,
            sources: vec![name.into()],
            kind: "request".into(),
            purpose: Purpose::Timing,
        };
        Entry {
            id: digest(&key).unwrap(),
            key,
            profile: ProfileChoice {
                preset: Some("base".into()),
                config: None,
                sha256: "0".repeat(64),
            },
            selected_parsers: vec![config::LogFormat::Classic],
            event_contract: 2,
            structural_contract: 1,
            resolution_contract: 2,
            provenance: Provenance {
                expected_sha256: "0".repeat(64),
                basis: "complete_current_sample_independent_assertions".into(),
            },
        }
    }
    #[test]
    fn concurrent_adds_preserve_both_and_stale_replacements_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let path = root.join("registry.json");
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = vec![];
        for name in ["a.jsonl", "b.jsonl"] {
            let root = root.clone();
            let path = path.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                Store::new(MappingScope::Project, root, Some(&path))
                    .unwrap()
                    .mutate(Some(entry(name)), None)
                    .is_ok()
            }));
        }
        for handle in handles {
            assert!(handle.join().unwrap());
        }
        let store = Store::new(MappingScope::Project, root.clone(), Some(&path)).unwrap();
        let registry = store.read().unwrap();
        assert_eq!(registry.entries.len(), 2);
        let prior = registry.entries[0].clone();
        let expected = digest(&prior).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = vec![];
        for preset in ["eyes", "service-api"] {
            let root = root.clone();
            let path = path.clone();
            let barrier = barrier.clone();
            let mut replacement = prior.clone();
            let expected = expected.clone();
            replacement.profile.preset = Some(preset.into());
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                Store::new(MappingScope::Project, root, Some(&path))
                    .unwrap()
                    .mutate(
                        Some(replacement.clone()),
                        Some((&replacement.id, &expected)),
                    )
                    .is_ok()
            }));
        }
        assert_eq!(
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .filter(|ok| *ok)
                .count(),
            1
        );
        assert_eq!(store.read().unwrap().entries.len(), 2);
        // A failed parse preserves the original malformed bytes.
        fs::write(&path, b"{invalid\n").unwrap();
        assert!(store.mutate(Some(entry("c.jsonl")), None).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{invalid\n");
    }
    #[test]
    fn lock_child_fixture() {
        let Some(path) = std::env::var_os("LOG_ANALYZER_TEST_NATIVE_LOCK") else {
            return;
        };
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.try_lock().unwrap();
        fs::write(PathBuf::from(&path).with_extension("ready"), b"ready").unwrap();
        loop {
            std::thread::park();
        }
    }
    #[test]
    fn process_exit_releases_native_lock_without_unlinking_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.lock");
        let ready = path.with_extension("ready");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "profile_mappings::tests::lock_child_fixture"])
            .env("LOG_ANALYZER_TEST_NATIVE_LOCK", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !ready.exists() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lock child did not become ready");
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let was_blocked = matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(was_blocked);
        file.try_lock().unwrap();
        assert!(path.exists());
    }
    #[test]
    fn readers_observe_whole_registry_during_atomic_replacements() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let path = root.join("registry.json");
        let store = Store::new(MappingScope::Project, root.clone(), Some(&path)).unwrap();
        store.mutate(Some(entry("a.jsonl")), None).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let ready = Arc::new(Barrier::new(2));
        let flag = stop.clone();
        let barrier = ready.clone();
        let read_path = path.clone();
        let reader = std::thread::spawn(move || {
            let mut reads = 0;
            barrier.wait();
            loop {
                let registry: Registry =
                    serde_json::from_slice(&fs::read(&read_path).unwrap()).unwrap();
                assert_eq!(registry.entries.len(), 1);
                reads += 1;
                if flag.load(Ordering::Acquire) {
                    break;
                }
            }
            reads
        });
        ready.wait();
        for i in 0..12 {
            let current = store.read().unwrap().entries.remove(0);
            let expected = digest(&current).unwrap();
            let mut changed = current.clone();
            changed.profile.preset = Some(format!("profile-{i}"));
            store
                .mutate(Some(changed), Some((&current.id, &expected)))
                .unwrap();
        }
        stop.store(true, Ordering::Release);
        assert!(reader.join().unwrap() > 0);
    }
    #[cfg(windows)]
    #[test]
    fn windows_replacement_waits_for_reader_and_preserves_old_file_on_timeout() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.json");
        fs::write(&path, b"old registry").unwrap();
        let blocked = || {
            OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(&path)
                .unwrap()
        };
        let candidate = || {
            let mut file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
            file.write_all(b"new registry").unwrap();
            file.as_file().sync_all().unwrap();
            file
        };
        let reader = blocked();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(75));
            drop(reader);
        });
        persist_registry(candidate(), &path).unwrap();
        release.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new registry");
        fs::write(&path, b"old registry").unwrap();
        let reader = blocked();
        let error = persist_registry(candidate(), &path).unwrap_err();
        assert!(matches!(error.raw_os_error(), Some(5 | 32 | 33)));
        assert_eq!(fs::read(&path).unwrap(), b"old registry");
        drop(reader);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[cfg(windows)]
    #[test]
    fn destination_guard_resolves_drive_prefixes_before_probing_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let expected = root.join("new").join("report.json");
        assert_eq!(
            destination_identity(&dir.path().join("new/report.json")).unwrap(),
            expected
        );
        assert_eq!(
            destination_identity(&root.join("new/report.json")).unwrap(),
            expected
        );
        assert_eq!(
            destination_identity(&root.join("other/../new/report.json")).unwrap(),
            expected
        );
        assert!(!root.join("new").exists());
    }
    #[test]
    fn postcommit_report_failure_is_explicit_and_inspection_failure_remains_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let report_path = dir.path().join("report.json");
        let staged = StagedReport::prepare(&report_path).unwrap();
        fs::create_dir(&report_path).unwrap();
        let store = Store::new(
            MappingScope::Project,
            dir.path().to_owned(),
            Some(&dir.path().join("registry.json")),
        )
        .unwrap();
        let committed = store.mutate(Some(entry("a.jsonl")), None).unwrap();
        let report = deliver_report(committed.report, true, Some(staged)).unwrap();
        assert_eq!(
            report["profile_mappings"]["mutation"]["status"],
            "committed"
        );
        assert_eq!(
            report["profile_mappings"]["report_save"]["status"],
            "failed"
        );
        assert_eq!(store.read().unwrap().entries.len(), 1);
        assert!(report_path.is_dir());
        let inspect_path = dir.path().join("inspection.json");
        let staged = StagedReport::prepare(&inspect_path).unwrap();
        fs::create_dir(&inspect_path).unwrap();
        assert!(
            deliver_report(
                store.view(&store.read().unwrap()).unwrap(),
                false,
                Some(staged)
            )
            .is_err()
        );
    }
}
