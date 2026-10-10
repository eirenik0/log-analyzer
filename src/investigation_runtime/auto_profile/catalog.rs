//! Bounded discovery with the same inheritance loader used by explicit configs.
use crate::{
    config::{self, AnalyzerConfig},
    evidence,
    processing::Budget,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 16;
const MAX_ENTRIES: usize = 256;
const MAX_DEPTH: usize = 4;
const FILE_BYTES: u64 = 64 * 1024;
const TOTAL_BYTES: u64 = 1024 * 1024;
const EFFECTIVE_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct Candidate {
    pub config: AnalyzerConfig,
    pub digest: String,
    pub analysis_digest: String,
    pub origins: Vec<Value>,
}

pub(super) struct Catalog {
    pub candidates: Vec<Candidate>,
    pub sources: Vec<PathBuf>,
    pub metadata: Value,
    pub complete: bool,
}

struct Discovery {
    paths: Vec<PathBuf>,
    sources: Vec<PathBuf>,
    diagnostics: Vec<Value>,
    complete: bool,
    entries: usize,
    bytes: u64,
}
impl Discovery {
    fn diagnostic(&mut self, path: &Path, reason: &str, incomplete: bool) {
        self.diagnostics
            .push(json!({"path":evidence::path_value(path),"reason":reason}));
        self.complete &= !incomplete;
    }
    fn walk(&mut self, dir: &Path, depth: usize, optional: bool, budget: &mut Budget) {
        if !budget.checkpoint("profile_discovery", 1) {
            self.complete = false;
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if optional && error.kind() == io::ErrorKind::NotFound => return,
            Err(_) => {
                self.diagnostic(dir, "directory_unavailable", true);
                return;
            }
        };
        let mut entries = entries
            .take(MAX_ENTRIES.saturating_sub(self.entries) + 1)
            .collect::<io::Result<Vec<_>>>();
        let Ok(ref mut entries) = entries else {
            self.diagnostic(dir, "directory_unavailable", true);
            return;
        };
        // Protect every encountered path before any limit/checkpoint can stop
        // discovery, including candidates that are never parsed.
        self.sources
            .extend(entries.iter().map(|entry| entry.path()).filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            }));
        self.entries += entries.len();
        if self.entries > MAX_ENTRIES {
            self.diagnostic(dir, "directory_entry_limit", true);
            return;
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if !budget.checkpoint("profile_discovery", 1) {
                self.complete = false;
                break;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                self.diagnostic(&path, "entry_unavailable", true);
                continue;
            };
            if kind.is_dir() {
                if depth == MAX_DEPTH {
                    self.diagnostic(&path, "directory_depth_limit", true);
                } else {
                    self.walk(&path, depth + 1, false, budget);
                }
            } else if kind.is_symlink() && fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
                self.diagnostic(&path, "symlink_directory_not_followed", false);
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
            {
                if self.paths.len() == MAX_FILES {
                    self.diagnostic(&path, "profile_file_limit", true);
                    return;
                }
                self.paths.push(path);
            }
        }
    }
    fn read(
        &mut self,
        path: &Path,
        dependencies: &mut Vec<Value>,
        budget: &mut Budget,
    ) -> io::Result<String> {
        self.sources.push(path.to_path_buf());
        if !budget.checkpoint("profile_discovery", 1) {
            return Err(io::Error::other("processing stopped"));
        }
        if !fs::metadata(path)?.is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        let file = fs::File::open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        let remaining = TOTAL_BYTES.saturating_sub(self.bytes).min(FILE_BYTES);
        if remaining == 0 {
            self.diagnostic(path, "configuration_total_byte_limit", true);
            return Err(io::Error::other("configuration byte limit"));
        }
        // Reserve before reading and parsing, including inherited sources. Scratch is
        // released by the caller once only the effective compiled config remains.
        if !budget.reserve(
            "profile_discovery",
            remaining.saturating_add(1).saturating_mul(16),
        ) {
            return Err(io::Error::other("processing stopped"));
        }
        let mut bytes = Vec::new();
        file.take(remaining + 1).read_to_end(&mut bytes)?;
        self.bytes += bytes.len() as u64;
        if !budget.checkpoint("profile_discovery", bytes.len() as u64) {
            return Err(io::Error::other("processing stopped"));
        }
        if bytes.len() as u64 > remaining {
            self.diagnostic(
                path,
                if remaining == FILE_BYTES {
                    "configuration_file_limit"
                } else {
                    "configuration_total_byte_limit"
                },
                true,
            );
            return Err(io::Error::other("configuration byte limit"));
        }
        dependencies
            .push(json!({"path":evidence::path_value(path),"sha256":evidence::digest(&bytes)}));
        String::from_utf8(bytes).map_err(|_| io::Error::other("configuration is not UTF-8"))
    }
}

pub(super) fn load(directory: &Path, optional: bool, budget: &mut Budget) -> Catalog {
    let mut discovery = Discovery {
        paths: Vec::new(),
        sources: Vec::new(),
        diagnostics: Vec::new(),
        complete: true,
        entries: 0,
        bytes: 0,
    };
    discovery.walk(directory, 0, optional, budget);
    discovery.paths.sort();
    let mut candidates = Vec::<Candidate>::new();
    let base_digest =
        evidence::profile_digest(config::default_config()).expect("profile serializes");
    let mut add = |config: AnalyzerConfig, mut origin: Value, budget: &mut Budget| -> bool {
        let bytes = serde_json::to_vec(&config).expect("profile serializes");
        if bytes.len() > EFFECTIVE_BYTES {
            return false;
        }
        let digest = evidence::profile_digest(&config).expect("profile serializes");
        if digest == base_digest {
            return true;
        }
        let mut analysis = serde_json::to_value(&config).expect("profile serializes");
        analysis.as_object_mut().unwrap().remove("profile_name");
        let analysis_digest = evidence::digest(analysis.to_string().as_bytes());
        origin["profile_sha256"] = json!(digest);
        origin["profile_name"] = json!(config.profile_name);
        if let Some(existing) = candidates
            .iter_mut()
            .find(|c| c.analysis_digest == analysis_digest)
        {
            existing.origins.push(origin);
        } else {
            if !budget.reserve("profile_discovery", (bytes.len() as u64).saturating_mul(16)) {
                return false;
            }
            candidates.push(Candidate {
                config,
                digest,
                analysis_digest,
                origins: vec![origin],
            });
        }
        true
    };
    for name in config::builtin_template_names()
        .iter()
        .filter(|name| **name != "base")
    {
        if !budget.checkpoint("profile_discovery", 1) {
            discovery.complete = false;
            break;
        }
        if !add(
            config::load_builtin_template(name).expect("valid embedded profile"),
            json!({"preset":name}),
            budget,
        ) {
            discovery.complete = false;
        }
    }
    for path in std::mem::take(&mut discovery.paths) {
        if budget.stop.is_some() {
            discovery.complete = false;
            break;
        }
        let memory_before = budget.memory_bytes;
        let mut dependencies = Vec::new();
        let loaded = config::load_config_from_path_with_reader(&path, &mut |source| {
            discovery.read(source, &mut dependencies, budget)
        });
        budget.memory_bytes = memory_before;
        match loaded {
            Ok((config, _)) => {
                if !add(
                    config,
                    json!({"config":evidence::path_value(&path),"dependencies":dependencies}),
                    budget,
                ) {
                    discovery.diagnostic(&path, "effective_configuration_limit", true);
                }
            }
            Err(_) => discovery.diagnostic(&path, "invalid_or_unreadable_configuration", true),
        }
    }
    let metadata = json!({"directory":evidence::path_value(directory),"complete":discovery.complete,
        "limits":{"toml_files":MAX_FILES,"directory_entries":MAX_ENTRIES,"directory_depth":MAX_DEPTH,
            "bytes_per_file":FILE_BYTES,"total_config_bytes":TOTAL_BYTES,"effective_config_bytes":EFFECTIVE_BYTES},
        "read_bytes":discovery.bytes,"diagnostics":discovery.diagnostics});
    Catalog {
        candidates,
        sources: discovery.sources,
        metadata,
        complete: discovery.complete,
    }
}
