//! The content director (technical overview §13; living-planet.md §1): game systems ask it for
//! generated text and choices (culture profiles, faction and settlement lore, codex entries)
//! as structured data, which they validate as their own types. Behind it sits a provider:
//! until the LLM gateway (Batch 8) that is the [`ReplayProvider`], answering from recorded
//! responses in `content/lore/`; the Anthropic and OpenAI providers will implement the same
//! [`Provider`] trait, so no caller changes.
//!
//! - A [`ContentRequest`] names a `kind` (what is asked for), a `key` (which one, e.g.
//!   `culture/hollin`) and a `context` (facts the answer must fit).
//! - [`ContentDirector::request`] returns a typed, validated [`Answer`]: from the cache, from
//!   the provider, or a fallback the caller supplies (when the provider has nothing or its
//!   answer doesn't validate), always saying which.
//! - Requests may block (a real model takes seconds): call them from worker threads, never on
//!   a frame. Answers are cached by [`ContentRequest::cache_key`]; once used, an answer becomes
//!   canon (the save keeps what it changed).
//!
//! Engine-independent: no rendering or engine dependencies.

mod replay;

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use replay::{RecordedResponse, ReplayError, ReplayProvider};

/// What a game system asks the content director for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContentRequest {
    /// What is asked for (`culture_profile`, `faction_lore`, `settlement_lore`, `codex`).
    pub kind: String,
    /// Which one: a path such as `culture/hollin` or `settlement/hollin/42`.
    pub key: String,
    /// Facts the answer must fit (names already chosen, tech tier, place, ...).
    #[serde(default)]
    pub context: Value,
}

impl ContentRequest {
    pub fn new(kind: impl Into<String>, key: impl Into<String>, context: Value) -> Self {
        Self {
            kind: kind.into(),
            key: key.into(),
            context,
        }
    }

    /// The cache key: kind, key and a hash of the context written with sorted object keys,
    /// so equal contexts hash alike however they were built.
    pub fn cache_key(&self) -> String {
        let mut canonical = String::new();
        write_canonical(&self.context, &mut canonical);
        format!(
            "{}:{}:{:016x}",
            self.kind,
            self.key,
            fnv1a(canonical.as_bytes())
        )
    }
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                write_canonical(&map[k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(v, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// FNV-1a: a small, stable hash (the same in every build and on every machine).
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Something that answers content requests with structured data.
pub trait Provider: Send + Sync {
    /// A short name for logs and [`Source`].
    fn name(&self) -> &str;
    /// The answer, or why there is none. May block.
    fn answer(&self, request: &ContentRequest) -> Result<Value, ProviderError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderError {
    /// The provider has no answer for this request.
    NoAnswer,
    /// The provider failed (network, quota, ...).
    Failed(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAnswer => write!(f, "no answer"),
            Self::Failed(e) => write!(f, "provider failed: {e}"),
        }
    }
}

/// Checks a typed answer beyond its shape (deserialising already checks fields and types):
/// rules such as "no spacefaring roles in a medieval culture".
pub trait Validate {
    fn validate(&self) -> Result<(), String>;
}

/// Where an answer came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Cache,
    Provider(String),
    /// The provider had nothing usable: the caller's fallback, with the reason.
    Fallback(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer<T> {
    pub value: T,
    pub source: Source,
}

/// The content director: a provider behind a cache and typed validation. Shared between
/// threads (`&self` everywhere).
pub struct ContentDirector {
    provider: Box<dyn Provider>,
    cache: Mutex<HashMap<String, Value>>,
}

impl ContentDirector {
    pub fn new(provider: Box<dyn Provider>) -> Self {
        Self {
            provider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

    /// Asks for `request` as a `T`: the cached answer if there is one, else the provider's if
    /// it parses and validates (then cached), else `fallback()` (not cached, so a later
    /// provider can still answer).
    pub fn request<T>(&self, request: &ContentRequest, fallback: impl FnOnce() -> T) -> Answer<T>
    where
        T: DeserializeOwned + Validate,
    {
        let key = request.cache_key();
        let cached = self.cache.lock().expect("content cache").get(&key).cloned();
        if let Some(value) = cached
            && let Ok(value) = parse::<T>(value)
        {
            return Answer {
                value,
                source: Source::Cache,
            };
        }
        let reason = match self.provider.answer(request) {
            Ok(raw) => match parse::<T>(raw.clone()) {
                Ok(value) => {
                    self.cache.lock().expect("content cache").insert(key, raw);
                    return Answer {
                        value,
                        source: Source::Provider(self.provider.name().to_string()),
                    };
                }
                Err(e) => format!(
                    "{} answered {} {}: {e}",
                    self.provider.name(),
                    request.kind,
                    request.key
                ),
            },
            Err(e) => format!(
                "{} for {} {}: {e}",
                self.provider.name(),
                request.kind,
                request.key
            ),
        };
        Answer {
            value: fallback(),
            source: Source::Fallback(reason),
        }
    }
}

fn parse<T: DeserializeOwned + Validate>(value: Value) -> Result<T, String> {
    let typed: T = serde_json::from_value(value).map_err(|e| format!("malformed: {e}"))?;
    typed.validate().map_err(|e| format!("invalid: {e}"))?;
    Ok(typed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(deny_unknown_fields)]
    struct Motto {
        text: String,
    }

    impl Validate for Motto {
        fn validate(&self) -> Result<(), String> {
            if self.text.is_empty() {
                return Err("empty motto".into());
            }
            Ok(())
        }
    }

    fn director(entries: Value) -> ContentDirector {
        let replay =
            ReplayProvider::from_json("test", &json!({ "responses": entries }).to_string())
                .unwrap();
        ContentDirector::new(Box::new(replay))
    }

    fn motto(key: &str) -> ContentRequest {
        ContentRequest::new("motto", key, json!({"culture": "hollin"}))
    }

    #[test]
    fn a_recorded_answer_is_returned_then_cached() {
        let d = director(json!([
            {"request": {"kind": "motto", "key": "faction/a"}, "response": {"text": "Hold fast"}}
        ]));
        let first = d.request(&motto("faction/a"), || Motto { text: "-".into() });
        assert_eq!(first.value.text, "Hold fast");
        assert_eq!(first.source, Source::Provider("replay:test".into()));
        let again = d.request(&motto("faction/a"), || Motto { text: "-".into() });
        assert_eq!(again.source, Source::Cache);
    }

    #[test]
    fn unknown_or_invalid_answers_fall_back() {
        let d = director(json!([
            {"request": {"kind": "motto", "key": "faction/bad"}, "response": {"text": ""}},
            {"request": {"kind": "motto", "key": "faction/odd"}, "response": {"words": "x"}}
        ]));
        for key in ["faction/none", "faction/bad", "faction/odd"] {
            let a = d.request(&motto(key), || Motto {
                text: "Fallback".into(),
            });
            assert_eq!(a.value.text, "Fallback");
            assert!(
                matches!(a.source, Source::Fallback(_)),
                "{key}: {:?}",
                a.source
            );
        }
    }

    #[test]
    fn patterns_answer_many_keys_from_a_pool_the_same_way_every_time() {
        let d = director(json!([
            {"request": {"kind": "motto", "key": "faction/special"}, "response": {"text": "Exact"}},
            {"request": {"kind": "motto", "key": "faction/*"}, "response": {"text": "One"}},
            {"request": {"kind": "motto", "key": "faction/*"}, "response": {"text": "Two"}},
            {"request": {"kind": "motto", "key": "*"}, "response": {"text": "Anything"}}
        ]));
        let get = |key: &str| {
            d.request(&motto(key), || Motto { text: "-".into() })
                .value
                .text
        };
        assert_eq!(get("faction/special"), "Exact");
        assert_eq!(get("other/thing"), "Anything");
        let picks: Vec<String> = (0..20).map(|i| get(&format!("faction/{i}"))).collect();
        assert!(picks.iter().any(|p| p == "One") && picks.iter().any(|p| p == "Two"));
        let fresh = director(json!([
            {"request": {"kind": "motto", "key": "faction/*"}, "response": {"text": "One"}},
            {"request": {"kind": "motto", "key": "faction/*"}, "response": {"text": "Two"}}
        ]));
        for (i, p) in picks.iter().enumerate() {
            let again = fresh.request(&motto(&format!("faction/{i}")), || Motto {
                text: "-".into(),
            });
            assert_eq!(&again.value.text, p, "the same key picks the same response");
        }
    }

    #[test]
    fn the_cache_key_follows_the_context() {
        let a = ContentRequest::new("k", "x", json!({"b": 1, "a": 2}));
        let b = ContentRequest::new("k", "x", json!({"a": 2, "b": 1}));
        let c = ContentRequest::new("k", "x", json!({"a": 3}));
        assert_eq!(a.cache_key(), b.cache_key());
        assert_ne!(a.cache_key(), c.cache_key());
    }
}
