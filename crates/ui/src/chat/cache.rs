use std::collections::HashMap;

use gpui_kit::SharedString;

/// Render state kept outside the virtual list, bounded to `cap` entries.
pub(crate) struct RenderCache<V> {
    cap: usize,
    tick: u64,
    entries: HashMap<SharedString, (u64, V)>,
}

impl<V> RenderCache<V> {
    pub(crate) fn new(cap: usize) -> Self {
        Self { cap, tick: 0, entries: HashMap::new() }
    }

    pub(crate) fn peek(&self, key: &SharedString) -> Option<&V> {
        self.entries.get(key).map(|(_, value)| value)
    }

    pub(crate) fn get(&mut self, key: &SharedString) -> Option<&mut V> {
        self.tick += 1;
        let (used, value) = self.entries.get_mut(key)?;
        *used = self.tick;
        Some(value)
    }

    pub(crate) fn insert(&mut self, key: SharedString, value: V) {
        self.tick += 1;
        self.entries.insert(key, (self.tick, value));
        if self.entries.len() > self.cap {
            // ponytail: O(n) LRU scan on overflow, switch to a linked map if profiles show it
            let oldest = self.entries.iter().min_by_key(|(_, (used, _))| *used);
            if let Some(oldest) = oldest.map(|(key, _)| key.clone()) {
                self.entries.remove(&oldest);
            }
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&SharedString, &V)> {
        self.entries.iter().map(|(key, (_, value))| (key, value))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/cache.rs"]
mod tests;
