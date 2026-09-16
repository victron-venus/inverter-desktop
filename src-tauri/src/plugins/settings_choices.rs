//! Bounded read-only choices published by a worker, never action authority.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    source: String,
    revision: String,
    offset: usize,
    complete: bool,
    options: Vec<Choice>,
}

#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    pub revision: Option<String>,
    pub sources: BTreeMap<String, Vec<Choice>>,
}

#[derive(Default)]
pub(super) struct Catalog {
    current: Snapshot,
    pending: BTreeMap<String, (String, Vec<Choice>)>,
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl Catalog {
    pub fn snapshot(&self) -> Snapshot {
        self.current.clone()
    }

    pub fn accept(&mut self, value: serde_json::Value) -> Result<(), ()> {
        let page: Page = serde_json::from_value(value).map_err(|_| ())?;
        if !token(&page.source)
            || !token(&page.revision)
            || page.options.len() > 64
            || page.offset > 4096
            || page.options.iter().any(|option| {
                option.value.is_empty()
                    || option.value.len() > 128
                    || option.label.len() > 128
                    || option.value.chars().any(char::is_control)
                    || option.label.chars().any(char::is_control)
            })
        {
            return Err(());
        }
        if page.offset == 0 {
            let sources: HashSet<_> = self
                .current
                .sources
                .keys()
                .chain(self.pending.keys())
                .collect();
            if !sources.contains(&page.source) && sources.len() >= 8 {
                return Err(());
            }
            self.pending
                .insert(page.source.clone(), (page.revision.clone(), Vec::new()));
        }
        let pending = self.pending.get_mut(&page.source).ok_or(())?;
        if pending.0 != page.revision
            || pending.1.len() != page.offset
            || pending.1.len() + page.options.len() > 4096
        {
            return Err(());
        }
        let mut values: HashSet<_> = pending
            .1
            .iter()
            .map(|choice| choice.value.as_str())
            .collect();
        for choice in &page.options {
            if !values.insert(&choice.value) {
                return Err(());
            }
        }
        pending.1.extend(page.options);
        if page.complete {
            let (_, choices) = self.pending.remove(&page.source).ok_or(())?;
            self.current.sources.insert(page.source, choices);
            self.current.revision = Some(page.revision);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn publication_is_atomic_and_cannot_duplicate_or_skip_entries() {
        let mut catalog = Catalog::default();
        let page = |offset, complete, value| json!({"source":"entities","revision":"one","offset":offset,"complete":complete,"options":[{"value":value,"label":"Device"}]});
        catalog.accept(page(0, false, "light.first")).unwrap();
        assert!(catalog.snapshot().sources.is_empty());
        assert!(catalog.accept(page(2, true, "light.second")).is_err());
        assert!(catalog.accept(page(1, true, "light.first")).is_err());
        catalog.accept(page(1, true, "light.second")).unwrap();
        assert_eq!(catalog.snapshot().sources["entities"].len(), 2);
        catalog.accept(page(0, false, "light.replacement")).unwrap();
        assert_eq!(
            catalog.snapshot().sources["entities"][0].value,
            "light.first"
        );
    }
}
