//! The replay provider: answers from recorded responses (`content/lore/*.json`), each file
//! `{"responses": [{"request": {"kind", "key"}, "response": {...}}, ...]}`.
//!
//! A recorded key is either exact (`culture/hollin`) or a pattern ending in `*`
//! (`settlement/hollin/*`, or `*` alone): a request is answered by its exact key if recorded,
//! else by the longest matching pattern. Several responses under one pattern form a pool, and
//! the request's key picks one by hash: the same key always gets the same response, different
//! keys spread over the pool (lore for generated settlements, recorded before their ids exist).

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ContentRequest, Provider, ProviderError, fnv1a};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedRequest {
    pub kind: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedResponse {
    pub request: RecordedRequest,
    pub response: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoreFile {
    #[serde(default)]
    #[allow(dead_code)]
    description: String,
    responses: Vec<RecordedResponse>,
}

#[derive(Debug)]
pub enum ReplayError {
    Io(String),
    Json {
        file: String,
        error: serde_json::Error,
    },
    /// The same exact key recorded twice.
    Duplicate {
        file: String,
        kind: String,
        key: String,
    },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read recorded responses: {e}"),
            Self::Json { file, error } => {
                write!(f, "recorded responses {file} are malformed: {error}")
            }
            Self::Duplicate { file, kind, key } => {
                write!(
                    f,
                    "{file}: {kind} {key} is recorded twice (only patterns may repeat)"
                )
            }
        }
    }
}

impl std::error::Error for ReplayError {}

/// Recorded responses by kind: exact keys, and pools under patterns.
#[derive(Clone, Debug, Default)]
pub struct ReplayProvider {
    name: String,
    exact: BTreeMap<(String, String), Value>,
    /// (kind, pattern without its `*`) -> pool.
    patterns: BTreeMap<(String, String), Vec<Value>>,
}

impl ReplayProvider {
    /// Loads every `*.json` file in `dir` (in name order).
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self, ReplayError> {
        let dir = dir.as_ref();
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| ReplayError::Io(format!("{}: {e}", dir.display())))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        let mut provider = Self {
            name: format!("replay:{}", dir.display()),
            ..Default::default()
        };
        for path in files {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| ReplayError::Io(format!("{}: {e}", path.display())))?;
            provider.add_file(&path.display().to_string(), &text)?;
        }
        Ok(provider)
    }

    /// From one file's text (tests).
    pub fn from_json(name: &str, text: &str) -> Result<Self, ReplayError> {
        let mut provider = Self {
            name: format!("replay:{name}"),
            ..Default::default()
        };
        provider.add_file(name, text)?;
        Ok(provider)
    }

    fn add_file(&mut self, file: &str, text: &str) -> Result<(), ReplayError> {
        let parsed: LoreFile = serde_json::from_str(text).map_err(|error| ReplayError::Json {
            file: file.to_string(),
            error,
        })?;
        for r in parsed.responses {
            let kind = r.request.kind;
            match r.request.key.strip_suffix('*') {
                Some(prefix) => self
                    .patterns
                    .entry((kind, prefix.to_string()))
                    .or_default()
                    .push(r.response),
                None => {
                    let key = r.request.key;
                    if self.exact.contains_key(&(kind.clone(), key.clone())) {
                        return Err(ReplayError::Duplicate {
                            file: file.to_string(),
                            kind,
                            key,
                        });
                    }
                    self.exact.insert((kind, key), r.response);
                }
            }
        }
        Ok(())
    }

    /// How many responses are recorded (exact and pooled).
    pub fn len(&self) -> usize {
        self.exact.len() + self.patterns.values().map(Vec::len).sum::<usize>()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Provider for ReplayProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn answer(&self, request: &ContentRequest) -> Result<Value, ProviderError> {
        if let Some(v) = self.exact.get(&(request.kind.clone(), request.key.clone())) {
            return Ok(v.clone());
        }
        let pool = self
            .patterns
            .iter()
            .filter(|((kind, prefix), _)| {
                *kind == request.kind && request.key.starts_with(prefix.as_str())
            })
            .max_by_key(|((_, prefix), _)| prefix.len())
            .map(|(_, pool)| pool)
            .ok_or(ProviderError::NoAnswer)?;
        let pick = (fnv1a(request.key.as_bytes()) % pool.len() as u64) as usize;
        Ok(pool[pick].clone())
    }
}
