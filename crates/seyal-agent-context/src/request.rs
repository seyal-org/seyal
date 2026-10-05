//! Consumer build request, token budget, and required-source contract (SPEC-013 §15).

use std::path::PathBuf;
use std::sync::Arc;

use crate::item::ContextItem;
use crate::scope::DiscoveryScope;
use crate::semantic::SemanticEnhancer;
use crate::source::SourceClass;

/// Consumer-supplied token budget (not frozen by the engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenBudget {
    pub max_tokens: u64,
}

impl TokenBudget {
    pub fn new(max_tokens: u64) -> Self {
        Self { max_tokens }
    }
}

/// Explicit required source for mandatory admission (§15 / §23.23 / §23.39).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredSource {
    pub relative_path: PathBuf,
    pub source_class: Option<SourceClass>,
}

impl RequiredSource {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self {
            relative_path: path.into(),
            source_class: None,
        }
    }
}

/// Build request for one immutable ContextBundle assembly.
#[derive(Clone)]
pub struct BuildRequest {
    pub scope: DiscoveryScope,
    pub budget: TokenBudget,
    pub required: Vec<RequiredSource>,
    pub pins: Vec<PathBuf>,
    /// Fixture / MemoryRecordRef stand-ins until #1273 lands.
    pub fixture_items: Vec<ContextItem>,
    /// Optional semantic enhancer (fail-closed / timeout → deterministic).
    pub semantic: Option<Arc<dyn SemanticEnhancer>>,
    /// When true, attempt LSP overlay path (fail-closed if unwired).
    pub enable_lsp_overlay: bool,
}

impl BuildRequest {
    pub fn new(scope: DiscoveryScope, budget: TokenBudget) -> Self {
        Self {
            scope,
            budget,
            required: Vec::new(),
            pins: Vec::new(),
            fixture_items: Vec::new(),
            semantic: None,
            enable_lsp_overlay: false,
        }
    }

    pub fn with_required(mut self, required: Vec<RequiredSource>) -> Self {
        self.required = required;
        self
    }

    pub fn with_pins(mut self, pins: Vec<PathBuf>) -> Self {
        self.pins = pins;
        self
    }

    pub fn with_fixtures(mut self, items: Vec<ContextItem>) -> Self {
        self.fixture_items = items;
        self
    }

    pub fn with_semantic(mut self, enhancer: Arc<dyn SemanticEnhancer>) -> Self {
        self.semantic = Some(enhancer);
        self
    }
}
