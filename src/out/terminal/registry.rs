use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::out::envelope::OutputEnvelope;
use crate::registry::OperationSpec;

use super::layout::TerminalDocument;
use super::renderer::TerminalRenderContext;

pub trait TerminalOperationRenderer {
    fn render(&self, envelope: &OutputEnvelope, cx: &TerminalRenderContext) -> TerminalDocument;
}

#[derive(Default)]
pub struct TerminalRendererRegistry {
    entries: Vec<TerminalRendererEntry>,
    violations: Vec<TerminalRendererRegistryViolation>,
}

struct TerminalRendererEntry {
    operation_id: &'static str,
    renderer: &'static dyn TerminalOperationRenderer,
}

impl TerminalRendererRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            violations: Vec::new(),
        }
    }

    pub fn register(
        mut self,
        operation_id: &'static str,
        renderer: &'static dyn TerminalOperationRenderer,
    ) -> Self {
        if self.contains(operation_id) {
            self.violations
                .push(TerminalRendererRegistryViolation::duplicate(operation_id));
        }
        self.entries.push(TerminalRendererEntry {
            operation_id,
            renderer,
        });
        self
    }

    pub fn contains(&self, operation_id: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.operation_id == operation_id)
    }

    pub fn get(&self, operation_id: &str) -> Option<&'static dyn TerminalOperationRenderer> {
        let mut matches = self
            .entries
            .iter()
            .filter(|entry| entry.operation_id == operation_id);
        let renderer = matches.next()?.renderer;
        matches.next().is_none().then_some(renderer)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn operation_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries.iter().map(|entry| entry.operation_id)
    }

    pub fn renderer_for(
        &self,
        operation_id: &str,
    ) -> Result<&'static dyn TerminalOperationRenderer, TerminalRendererRegistryError> {
        self.get(operation_id).ok_or_else(|| {
            TerminalRendererRegistryError::from_violation(
                TerminalRendererRegistryViolation::missing(operation_id.to_owned()),
            )
        })
    }

    pub fn validate_against_operations(
        &self,
        operations: &[OperationSpec],
    ) -> Result<(), TerminalRendererRegistryError> {
        self.validate_operation_ids(operations.iter().map(|operation| operation.operation_id))
    }

    pub fn validate_operation_ids<'a>(
        &self,
        expected: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), TerminalRendererRegistryError> {
        let expected = expected.into_iter().collect::<BTreeSet<_>>();
        let mut counts = BTreeMap::<&str, usize>::new();
        for operation_id in self.operation_ids() {
            *counts.entry(operation_id).or_default() += 1;
        }
        let mut violations = self.violations.clone();
        for operation_id in expected {
            match counts.get(operation_id).copied().unwrap_or(0) {
                0 => {
                    violations.push(TerminalRendererRegistryViolation::missing(
                        operation_id.to_owned(),
                    ));
                }
                1 => {}
                count => {
                    violations.push(TerminalRendererRegistryViolation::duplicate_count(
                        operation_id.to_owned(),
                        count,
                    ));
                }
            }
        }
        violations.sort();
        violations.dedup();
        if violations.is_empty() {
            Ok(())
        } else {
            Err(TerminalRendererRegistryError { violations })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TerminalRendererRegistryViolation {
    DuplicateOperation { operation_id: String, count: usize },
    MissingOperation { operation_id: String },
}

impl TerminalRendererRegistryViolation {
    fn duplicate(operation_id: impl Into<String>) -> Self {
        Self::DuplicateOperation {
            operation_id: operation_id.into(),
            count: 2,
        }
    }

    fn duplicate_count(operation_id: impl Into<String>, count: usize) -> Self {
        Self::DuplicateOperation {
            operation_id: operation_id.into(),
            count,
        }
    }

    fn missing(operation_id: impl Into<String>) -> Self {
        Self::MissingOperation {
            operation_id: operation_id.into(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::DuplicateOperation { .. } => "duplicate",
            Self::MissingOperation { .. } => "missing",
        }
    }

    pub fn operation_id(&self) -> &str {
        match self {
            Self::DuplicateOperation { operation_id, .. }
            | Self::MissingOperation { operation_id } => operation_id,
        }
    }

    pub fn count(&self) -> Option<usize> {
        match self {
            Self::DuplicateOperation { count, .. } => Some(*count),
            Self::MissingOperation { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRendererRegistryError {
    violations: Vec<TerminalRendererRegistryViolation>,
}

impl TerminalRendererRegistryError {
    pub fn from_violation(violation: TerminalRendererRegistryViolation) -> Self {
        Self {
            violations: vec![violation],
        }
    }

    pub fn violations(&self) -> &[TerminalRendererRegistryViolation] {
        &self.violations
    }
}

impl fmt::Display for TerminalRendererRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("terminal renderer registry invariant failed")?;
        for violation in &self.violations {
            match violation {
                TerminalRendererRegistryViolation::DuplicateOperation {
                    operation_id,
                    count,
                } => write!(
                    formatter,
                    "; duplicate renderer for {operation_id} ({count})"
                )?,
                TerminalRendererRegistryViolation::MissingOperation { operation_id } => {
                    write!(formatter, "; missing renderer for {operation_id}")?
                }
            }
        }
        Ok(())
    }
}

pub fn terminal_renderer_registry() -> TerminalRendererRegistry {
    let registry = TerminalRendererRegistry::new();
    let registry = crate::out::terminal::renderers::workspace::register(registry);
    let registry = crate::out::terminal::renderers::health::register(registry);
    let registry = crate::out::terminal::renderers::config::register(registry);
    let registry = crate::out::terminal::renderers::account::register(registry);
    let registry = crate::out::terminal::renderers::runtime::register(registry);
    let registry = crate::out::terminal::renderers::store::register(registry);
    let registry = crate::out::terminal::renderers::sync::register(registry);
    let registry = crate::out::terminal::renderers::farm::register(registry);
    let registry = crate::out::terminal::renderers::listing::register(registry);
    let registry = crate::out::terminal::renderers::market::register(registry);
    let registry = crate::out::terminal::renderers::basket::register(registry);
    let registry = crate::out::terminal::renderers::trade::register(registry);
    crate::out::terminal::renderers::validation::register(registry)
}

#[cfg(test)]
mod tests {
    use crate::out::terminal::layout::{TerminalDocument, TerminalHeader, TerminalSymbol};
    use crate::registry::OPERATION_REGISTRY;

    use super::*;

    struct TestRenderer;

    impl TerminalOperationRenderer for TestRenderer {
        fn render(
            &self,
            _envelope: &OutputEnvelope,
            _cx: &TerminalRenderContext,
        ) -> TerminalDocument {
            TerminalDocument::new(TerminalHeader::new(TerminalSymbol::Success, "ok"))
        }
    }

    static TEST_RENDERER: TestRenderer = TestRenderer;

    #[test]
    fn registers_operation_renderers() {
        let registry = TerminalRendererRegistry::new().register("workspace.get", &TEST_RENDERER);

        assert!(registry.contains("workspace.get"));
        assert_eq!(registry.len(), 1);
        assert!(registry.get("workspace.get").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn duplicate_operation_renderer_registration_fails() {
        let registry = TerminalRendererRegistry::new()
            .register("workspace.get", &TEST_RENDERER)
            .register("workspace.get", &TEST_RENDERER);
        let error = registry
            .validate_operation_ids(["workspace.get"])
            .expect_err("duplicate renderer registration should fail");

        assert_eq!(
            error.violations(),
            &[TerminalRendererRegistryViolation::duplicate_count(
                "workspace.get",
                2
            )]
        );
        assert!(registry.get("workspace.get").is_none());
    }

    #[test]
    fn missing_operation_renderer_registration_fails() {
        let error = TerminalRendererRegistry::new()
            .validate_operation_ids(["workspace.get"])
            .expect_err("missing renderer registration should fail");

        assert_eq!(
            error.violations(),
            &[TerminalRendererRegistryViolation::missing("workspace.get")]
        );
    }

    #[test]
    fn registry_covers_registered_operations() {
        let registry = terminal_renderer_registry();
        let expected = OPERATION_REGISTRY
            .iter()
            .map(|operation| operation.operation_id)
            .collect::<BTreeSet<_>>();
        let actual = registry.operation_ids().collect::<BTreeSet<_>>();

        assert_eq!(registry.len(), OPERATION_REGISTRY.len());
        assert_eq!(actual, expected);
        registry
            .validate_against_operations(OPERATION_REGISTRY)
            .expect("terminal registry covers operation registry");
        for operation_id in expected {
            assert!(
                registry.contains(operation_id),
                "missing terminal renderer for {operation_id}"
            );
        }
    }
}
