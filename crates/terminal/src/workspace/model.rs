use gpui_kit::{Axis, SharedString};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GroupId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PaneId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SplitId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeConnector {
    Single,
    First,
    Middle,
    Last,
}

impl TreeConnector {
    pub(crate) fn is_split_child(self) -> bool {
        matches!(self, Self::Middle | Self::Last)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RemoveResult {
    Removed { promoted_first_leaf: PaneId },
    RemovedLast,
    NotFound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseOutcome {
    Focused(PaneId),
    HideWorkspace { replacement_pane: PaneId },
    NotFound,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SplitNode {
    Leaf(PaneId),
    Split { id: SplitId, axis: Axis, ratio: f32, first: Box<SplitNode>, second: Box<SplitNode> },
}

impl SplitNode {
    pub(crate) const MIN_RATIO: f32 = 0.15;
    pub(crate) const MAX_RATIO: f32 = 0.85;
    pub(crate) const DEFAULT_RATIO: f32 = 0.5;

    pub(crate) fn clamp_ratio(ratio: f32) -> f32 {
        if ratio.is_finite() {
            ratio.clamp(Self::MIN_RATIO, Self::MAX_RATIO)
        } else {
            Self::DEFAULT_RATIO
        }
    }

    pub(crate) fn first_leaf(&self) -> PaneId {
        match self {
            Self::Leaf(pane_id) => *pane_id,
            Self::Split { first, .. } => first.first_leaf(),
        }
    }

    pub(crate) fn leaves(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Leaf(pane_id) => out.push(*pane_id),
            Self::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    pub(crate) fn contains_leaf(&self, target: PaneId) -> bool {
        match self {
            Self::Leaf(pane_id) => *pane_id == target,
            Self::Split { first, second, .. } => {
                first.contains_leaf(target) || second.contains_leaf(target)
            }
        }
    }

    pub(crate) fn split_leaf(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        split_id: SplitId,
        axis: Axis,
    ) -> bool {
        match self {
            Self::Leaf(pane_id) if *pane_id == target => {
                let existing = *pane_id;
                *self = Self::Split {
                    id: split_id,
                    axis,
                    ratio: Self::DEFAULT_RATIO,
                    first: Box::new(Self::Leaf(existing)),
                    second: Box::new(Self::Leaf(new_pane)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target, new_pane, split_id, axis)
                    || second.split_leaf(target, new_pane, split_id, axis)
            }
        }
    }

    pub(crate) fn remove_leaf(&mut self, target: PaneId) -> RemoveResult {
        match self {
            Self::Leaf(pane_id) => {
                if *pane_id == target {
                    RemoveResult::RemovedLast
                } else {
                    RemoveResult::NotFound
                }
            }
            Self::Split { first, second, .. } => {
                if matches!(first.as_ref(), Self::Leaf(id) if *id == target) {
                    let promoted = (**second).clone();
                    let promoted_first_leaf = promoted.first_leaf();
                    *self = promoted;
                    return RemoveResult::Removed { promoted_first_leaf };
                }
                if matches!(second.as_ref(), Self::Leaf(id) if *id == target) {
                    let promoted = (**first).clone();
                    let promoted_first_leaf = promoted.first_leaf();
                    *self = promoted;
                    return RemoveResult::Removed { promoted_first_leaf };
                }
                match first.remove_leaf(target) {
                    RemoveResult::NotFound => second.remove_leaf(target),
                    res => res,
                }
            }
        }
    }

    pub(crate) fn set_ratio(&mut self, id: SplitId, ratio: f32) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::Split { id: split_id, ratio: current_ratio, first, second, .. } => {
                if *split_id == id {
                    *current_ratio = Self::clamp_ratio(ratio);
                    true
                } else {
                    first.set_ratio(id, ratio) || second.set_ratio(id, ratio)
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn ratio_of(&self, id: SplitId) -> Option<f32> {
        match self {
            Self::Leaf(_) => None,
            Self::Split { id: split_id, ratio, first, second, .. } => {
                if *split_id == id {
                    Some(*ratio)
                } else {
                    first.ratio_of(id).or_else(|| second.ratio_of(id))
                }
            }
        }
    }

    pub(crate) fn tree_rows(&self) -> Vec<(PaneId, TreeConnector)> {
        let leaves = self.leaves();
        let len = leaves.len();
        leaves
            .into_iter()
            .enumerate()
            .map(|(idx, pane_id)| {
                let connector = if len <= 1 {
                    TreeConnector::Single
                } else if idx == 0 {
                    TreeConnector::First
                } else if idx + 1 == len {
                    TreeConnector::Last
                } else {
                    TreeConnector::Middle
                };
                (pane_id, connector)
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupModel {
    pub id: GroupId,
    pub custom_title: Option<SharedString>,
    pub root: SplitNode,
    pub active_pane: PaneId,
}

impl GroupModel {
    pub(crate) fn new(id: GroupId, initial_pane: PaneId) -> Self {
        Self {
            id,
            custom_title: None,
            root: SplitNode::Leaf(initial_pane),
            active_pane: initial_pane,
        }
    }

    pub(crate) fn pane_count(&self) -> usize {
        self.root.leaves().len()
    }

    pub(crate) fn leaves(&self) -> Vec<PaneId> {
        self.root.leaves()
    }

    pub(crate) fn contains_pane(&self, pane: PaneId) -> bool {
        self.root.contains_leaf(pane)
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceModel {
    pub(crate) groups: Vec<GroupModel>,
    pub(crate) active_group: GroupId,
    pub sidebar_open: bool,
    pub(crate) maximized: bool,
    next_group_id: u64,
    next_pane_id: u64,
    next_split_id: u64,
}

impl Default for WorkspaceModel {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceModel {
    pub(crate) fn new() -> Self {
        let initial_group_id = GroupId(1);
        let initial_pane_id = PaneId(1);
        let initial_group = GroupModel::new(initial_group_id, initial_pane_id);
        Self {
            groups: vec![initial_group],
            active_group: initial_group_id,
            sidebar_open: true,
            maximized: false,
            next_group_id: 2,
            next_pane_id: 2,
            next_split_id: 1,
        }
    }

    fn alloc_group_id(&mut self) -> GroupId {
        let id = GroupId(self.next_group_id);
        self.next_group_id += 1;
        id
    }

    fn alloc_pane_id(&mut self) -> PaneId {
        let id = PaneId(self.next_pane_id);
        self.next_pane_id += 1;
        id
    }

    fn alloc_split_id(&mut self) -> SplitId {
        let id = SplitId(self.next_split_id);
        self.next_split_id += 1;
        id
    }

    pub(crate) fn active_group_id(&self) -> GroupId {
        self.active_group
    }

    pub(crate) fn active_pane_id(&self) -> PaneId {
        self.active_group().active_pane
    }

    pub(crate) fn active_group(&self) -> &GroupModel {
        self.groups.iter().find(|g| g.id == self.active_group).unwrap_or(&self.groups[0])
    }

    pub(crate) fn active_group_mut(&mut self) -> &mut GroupModel {
        let active_id = self.active_group;
        let idx = self.groups.iter().position(|g| g.id == active_id).unwrap_or(0);
        &mut self.groups[idx]
    }

    pub(crate) fn group(&self, id: GroupId) -> Option<&GroupModel> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub(crate) fn group_mut(&mut self, id: GroupId) -> Option<&mut GroupModel> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub(crate) fn group_of_pane(&self, pane: PaneId) -> Option<GroupId> {
        self.groups.iter().find(|g| g.contains_pane(pane)).map(|g| g.id)
    }

    pub(crate) fn new_group(&mut self) -> (GroupId, PaneId) {
        let group_id = self.alloc_group_id();
        let pane_id = self.alloc_pane_id();
        self.groups.push(GroupModel::new(group_id, pane_id));
        self.active_group = group_id;
        (group_id, pane_id)
    }

    pub(crate) fn select_group(&mut self, id: GroupId) -> Option<PaneId> {
        let pane = self.group(id)?.active_pane;
        self.active_group = id;
        Some(pane)
    }

    pub(crate) fn focus_pane(&mut self, pane: PaneId) -> bool {
        if let Some(group) = self.groups.iter_mut().find(|g| g.contains_pane(pane)) {
            group.active_pane = pane;
            self.active_group = group.id;
            true
        } else {
            false
        }
    }

    pub(crate) fn split_pane(&mut self, target: PaneId, axis: Axis) -> Option<PaneId> {
        let group_idx = self.groups.iter().position(|g| g.contains_pane(target))?;
        let new_pane = self.alloc_pane_id();
        let split_id = self.alloc_split_id();
        if self.groups[group_idx].root.split_leaf(target, new_pane, split_id, axis) {
            self.groups[group_idx].active_pane = new_pane;
            self.active_group = self.groups[group_idx].id;
            Some(new_pane)
        } else {
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn split_active(&mut self, axis: Axis) -> PaneId {
        let target = self.active_pane_id();
        self.split_pane(target, axis).unwrap_or(target)
    }

    pub(crate) fn set_split_ratio(&mut self, split_id: SplitId, ratio: f32) -> bool {
        for group in &mut self.groups {
            if group.root.set_ratio(split_id, ratio) {
                return true;
            }
        }
        false
    }

    pub(crate) fn reset_split_ratio(&mut self, split_id: SplitId) -> bool {
        self.set_split_ratio(split_id, SplitNode::DEFAULT_RATIO)
    }

    pub(crate) fn close_pane(&mut self, target: PaneId) -> CloseOutcome {
        let Some(group_idx) = self.groups.iter().position(|g| g.contains_pane(target)) else {
            return CloseOutcome::NotFound;
        };

        let remove_res = self.groups[group_idx].root.remove_leaf(target);
        match remove_res {
            RemoveResult::NotFound => CloseOutcome::NotFound,
            RemoveResult::Removed { promoted_first_leaf } => {
                let group = &mut self.groups[group_idx];
                if group.active_pane == target || !group.root.contains_leaf(group.active_pane) {
                    group.active_pane = promoted_first_leaf;
                }
                CloseOutcome::Focused(self.active_pane_id())
            }
            RemoveResult::RemovedLast => {
                let group_id = self.groups[group_idx].id;
                self.close_group(group_id)
            }
        }
    }

    pub(crate) fn close_group(&mut self, group_id: GroupId) -> CloseOutcome {
        let Some(group_idx) = self.groups.iter().position(|g| g.id == group_id) else {
            return CloseOutcome::NotFound;
        };

        self.groups.remove(group_idx);
        if self.groups.is_empty() {
            let (_, replacement_pane) = self.new_group();
            CloseOutcome::HideWorkspace { replacement_pane }
        } else {
            if self.active_group == group_id {
                let next_idx = group_idx.min(self.groups.len() - 1);
                self.active_group = self.groups[next_idx].id;
            }
            CloseOutcome::Focused(self.active_pane_id())
        }
    }

    pub(crate) fn focus_next_pane(&mut self) -> PaneId {
        let group = self.active_group_mut();
        let leaves = group.root.leaves();
        if leaves.is_empty() {
            return group.active_pane;
        }
        let current_idx = leaves.iter().position(|&p| p == group.active_pane).unwrap_or(0);
        let next_pane = leaves[(current_idx + 1) % leaves.len()];
        group.active_pane = next_pane;
        next_pane
    }

    pub(crate) fn focus_prev_pane(&mut self) -> PaneId {
        let group = self.active_group_mut();
        let leaves = group.root.leaves();
        if leaves.is_empty() {
            return group.active_pane;
        }
        let current_idx = leaves.iter().position(|&p| p == group.active_pane).unwrap_or(0);
        let prev_pane = leaves[(current_idx + leaves.len() - 1) % leaves.len()];
        group.active_pane = prev_pane;
        prev_pane
    }

    pub(crate) fn focus_next_group(&mut self) -> PaneId {
        if self.groups.is_empty() {
            return PaneId(1);
        }
        let current_idx = self.groups.iter().position(|g| g.id == self.active_group).unwrap_or(0);
        let next_idx = (current_idx + 1) % self.groups.len();
        self.active_group = self.groups[next_idx].id;
        self.groups[next_idx].active_pane
    }

    pub(crate) fn focus_prev_group(&mut self) -> PaneId {
        if self.groups.is_empty() {
            return PaneId(1);
        }
        let current_idx = self.groups.iter().position(|g| g.id == self.active_group).unwrap_or(0);
        let prev_idx = (current_idx + self.groups.len() - 1) % self.groups.len();
        self.active_group = self.groups[prev_idx].id;
        self.groups[prev_idx].active_pane
    }

    pub(crate) fn set_group_title(
        &mut self,
        group_id: GroupId,
        title: Option<SharedString>,
    ) -> bool {
        if let Some(group) = self.group_mut(group_id) {
            group.custom_title = title.and_then(|t| {
                let trimmed = t.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(SharedString::from(trimmed.to_string()))
                }
            });
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/model.rs"]
mod tests;
