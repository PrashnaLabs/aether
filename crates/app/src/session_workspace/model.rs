use aether_core::TileLayout;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) type PaneId = u64;
pub(crate) type WindowId = u64;
pub(crate) const DIVIDER: f32 = 4.;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) enum SplitAxis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Direction {
    Left,
    Right,
    Up,
    Down,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum DockSide {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub(crate) fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub(crate) fn sanitize(self, size: [f32; 2]) -> Self {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        let w = finite(self.w, 400.).max(320.).min(size[0].max(320.));
        let h = finite(self.h, 300.).max(240.).min(size[1].max(240.));
        Self {
            x: finite(self.x, 0.).clamp(0., (size[0] - w).max(0.)),
            y: finite(self.y, 0.).clamp(0., (size[1] - 32.).max(0.)),
            w,
            h,
        }
    }
}

pub(crate) fn restore_window_rect(saved: Rect, displays: &[Rect]) -> Rect {
    let valid = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
    let saved = Rect::new(
        valid(saved.x, 0.),
        valid(saved.y, 0.),
        valid(saved.w, 900.).max(320.),
        valid(saved.h, 700.).max(240.),
    );
    let overlap = |display: &&Rect| {
        ((saved.x + saved.w).min(display.x + display.w) - saved.x.max(display.x)).max(0.)
            * ((saved.y + saved.h).min(display.y + display.h) - saved.y.max(display.y)).max(0.)
    };
    let display = displays
        .iter()
        .max_by(|a, b| overlap(a).total_cmp(&overlap(b)))
        .copied()
        .unwrap_or(Rect::new(0., 0., 1440., 900.));
    let w = saved.w.min(display.w.max(320.));
    let h = saved.h.min(display.h.max(240.));
    Rect::new(
        saved.x.clamp(display.x, (display.x + display.w - w).max(display.x)),
        saved.y.clamp(display.y, (display.y + display.h - h).max(display.y)),
        w,
        h,
    )
}

pub(crate) fn split_sizes(total: f32, a: f32, b: f32, ratio: f32) -> Option<(f32, f32)> {
    let usable = total - DIVIDER;
    if !usable.is_finite() || usable < a + b {
        return None;
    }
    let ratio = if ratio.is_finite() { ratio.clamp(0.15, 0.85) } else { 0.5 };
    let first = (usable * ratio).clamp(a, usable - b);
    Some((first, usable - first))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum DockNode {
    Leaf(PaneId),
    Split { id: u64, axis: SplitAxis, ratio: f32, first: Box<Self>, second: Box<Self> },
}
impl DockNode {
    pub(crate) fn leaves(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut result = first.leaves();
                result.extend(second.leaves());
                result
            }
        }
    }
    pub(crate) fn minimum(&self) -> [f32; 2] {
        match self {
            Self::Leaf(_) => [320., 240.],
            Self::Split { axis, first, second, .. } => {
                let (a, b) = (first.minimum(), second.minimum());
                match axis {
                    SplitAxis::Horizontal => [a[0] + DIVIDER + b[0], a[1].max(b[1])],
                    SplitAxis::Vertical => [a[0].max(b[0]), a[1] + DIVIDER + b[1]],
                }
            }
        }
    }
    fn insert(
        &mut self,
        target: PaneId,
        new: PaneId,
        id: u64,
        axis: SplitAxis,
        before: bool,
    ) -> bool {
        match self {
            Self::Leaf(p) if *p == target => {
                let (a, b) = if before { (new, target) } else { (target, new) };
                *self = Self::Split {
                    id,
                    axis,
                    ratio: 0.5,
                    first: Box::new(Self::Leaf(a)),
                    second: Box::new(Self::Leaf(b)),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.insert(target, new, id, axis, before)
                    || second.insert(target, new, id, axis, before)
            }
            _ => false,
        }
    }
    pub(crate) fn remove(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Leaf(id) => (id != target).then_some(Self::Leaf(id)),
            Self::Split { id, axis, ratio, first, second } => {
                match (first.remove(target), second.remove(target)) {
                    (Some(a), Some(b)) => Some(Self::Split {
                        id,
                        axis,
                        ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (a, b) => a.or(b),
                }
            }
        }
    }
    fn swap(&mut self, a: PaneId, b: PaneId) {
        match self {
            Self::Leaf(p) if *p == a => *p = b,
            Self::Leaf(p) if *p == b => *p = a,
            Self::Leaf(_) => {}
            Self::Split { first, second, .. } => {
                first.swap(a, b);
                second.swap(a, b);
            }
        }
    }
    pub(crate) fn set_ratio(&mut self, target: u64, value: f32) {
        if let Self::Split { id, ratio, first, second, .. } = self {
            if *id == target && value.is_finite() {
                *ratio = value.clamp(0.15, 0.85);
            } else {
                first.set_ratio(target, value);
                second.set_ratio(target, value);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FloatingPane {
    pub pane: PaneId,
    pub bounds: Rect,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct WindowLayout {
    pub bounds: Option<Rect>,
    pub stack: Vec<PaneId>,
    pub root: Option<DockNode>,
    pub floats: Vec<FloatingPane>,
    pub focused: Option<PaneId>,
    pub maximized: Option<PaneId>,
    #[serde(skip)]
    pub tiled: Option<(TileLayout, f32, Vec<PaneId>)>,
    // monocle tab; outlives focus moving to a float
    #[serde(skip)]
    pub shown: Option<PaneId>,
}
impl WindowLayout {
    pub(crate) fn panes(&self) -> Vec<PaneId> {
        let mut ids = self.root.as_ref().map(DockNode::leaves).unwrap_or_default();
        ids.extend(self.floats.iter().map(|f| f.pane));
        ids
    }
    fn remove(&mut self, pane: PaneId) {
        self.root = self.root.take().and_then(|r| r.remove(pane));
        self.floats.retain(|f| f.pane != pane);
        self.stack.retain(|p| *p != pane);
        if self.focused == Some(pane) {
            self.focused = self.panes().first().copied();
        }
        if self.maximized == Some(pane) {
            self.maximized = None;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WorkspaceModel {
    pub windows: BTreeMap<WindowId, WindowLayout>,
    pub panes: BTreeMap<PaneId, i64>,
    next_id: u64,
}
impl Default for WorkspaceModel {
    fn default() -> Self {
        Self {
            windows: BTreeMap::from([(0, WindowLayout::default())]),
            panes: BTreeMap::new(),
            next_id: 1,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Transfer {
    pane: PaneId,
    chat: i64,
    source: WindowId,
    destination: WindowId,
}
impl WorkspaceModel {
    pub(crate) fn prepare_transfer(
        &self,
        pane: PaneId,
        destination: WindowId,
    ) -> Result<Transfer, &'static str> {
        Ok(Transfer {
            pane,
            chat: *self.panes.get(&pane).ok_or("Session no longer exists")?,
            source: self.window_of(pane).ok_or("Pane no longer exists")?,
            destination,
        })
    }
    pub(crate) fn commit_transfer(&mut self, ticket: Transfer) -> Result<(), &'static str> {
        if self.panes.get(&ticket.pane) != Some(&ticket.chat)
            || self.window_of(ticket.pane) != Some(ticket.source)
        {
            return Err("Session moved or closed during transfer");
        }
        self.transfer(ticket.pane, ticket.destination)
    }
    pub(crate) fn repair(&mut self, known: &std::collections::HashSet<i64>, size: [f32; 2]) {
        let mut seen = std::collections::HashSet::new();
        let mut seen_panes = std::collections::HashSet::new();
        for layout in self.windows.values_mut() {
            for pane in layout.panes() {
                let keep =
                    self.panes.get(&pane).is_some_and(|c| known.contains(c) && seen.insert(*c))
                        && seen_panes.insert(pane);
                if !keep {
                    layout.remove(pane);
                }
            }
            if !layout.focused.is_some_and(|p| layout.panes().contains(&p)) {
                layout.focused = layout.panes().first().copied();
            }
            if !layout.maximized.is_some_and(|p| layout.panes().contains(&p)) {
                layout.maximized = None;
            }
            for float in &mut layout.floats {
                float.bounds = float.bounds.sanitize(size);
            }
            layout.stack.retain(|p| layout.floats.iter().any(|f| f.pane == *p));
            let missing: Vec<_> = layout
                .floats
                .iter()
                .filter(|f| !layout.stack.contains(&f.pane))
                .map(|f| f.pane)
                .collect();
            layout.stack.extend(missing);
            fn ratios(node: &mut DockNode) {
                if let DockNode::Split { ratio, first, second, .. } = node {
                    *ratio = if ratio.is_finite() { ratio.clamp(0.15, 0.85) } else { 0.5 };
                    ratios(first);
                    ratios(second);
                }
            }
            if let Some(root) = &mut layout.root {
                ratios(root);
            }
        }
        self.panes.retain(|p, _| self.windows.values().any(|w| w.panes().contains(p)));
        self.windows.retain(|id, w| *id == 0 || !w.panes().is_empty());
        self.windows.entry(0).or_default();
        fn max_split(node: &DockNode) -> u64 {
            match node {
                DockNode::Leaf(p) => *p,
                DockNode::Split { id, first, second, .. } => {
                    *id.max(&max_split(first)).max(&max_split(second))
                }
            }
        }
        let max = self
            .windows
            .iter()
            .map(|(id, w)| w.root.as_ref().map(max_split).unwrap_or(0).max(*id))
            .chain(self.panes.keys().copied())
            .max()
            .unwrap_or(0);
        // A corrupt counter cannot make future allocations overflow.
        self.next_id = max.saturating_add(1).max(1);
    }
    pub(crate) fn alloc(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).unwrap_or(1);
        id
    }
    pub(crate) fn window_of(&self, pane: PaneId) -> Option<WindowId> {
        self.windows.iter().find_map(|(id, w)| w.panes().contains(&pane).then_some(*id))
    }
    pub(crate) fn pane_for(&self, chat: i64) -> Option<PaneId> {
        self.panes.iter().find_map(|(p, c)| (*c == chat).then_some(*p))
    }
    pub(crate) fn focus(&mut self, pane: PaneId) {
        if let Some(w) = self.window_of(pane) {
            let layout = self.windows.get_mut(&w).unwrap();
            layout.focused = Some(pane);
            if layout.maximized.is_some_and(|p| p != pane) {
                layout.maximized = None;
            }
            if layout.floats.iter().any(|f| f.pane == pane) {
                layout.stack.retain(|p| *p != pane);
                layout.stack.push(pane);
            }
        }
    }
    pub(crate) fn open(&mut self, chat: i64, window: WindowId) -> PaneId {
        if let Some(p) = self.pane_for(chat) {
            self.focus(p);
            return p;
        }
        let p = self.alloc();
        let s = self.alloc();
        self.panes.insert(p, chat);
        let w = self.windows.entry(window).or_default();
        w.root = Some(match w.root.take() {
            None => DockNode::Leaf(p),
            Some(old) => DockNode::Split {
                id: s,
                axis: SplitAxis::Horizontal,
                ratio: 0.5,
                first: Box::new(old),
                second: Box::new(DockNode::Leaf(p)),
            },
        });
        w.focused = Some(p);
        p
    }
    pub(crate) fn can_split(
        &self,
        target: PaneId,
        axis: SplitAxis,
        size: [f32; 2],
    ) -> Result<(), &'static str> {
        let needed = match axis {
            SplitAxis::Horizontal => [644., 240.],
            SplitAxis::Vertical => [320., 484.],
        };
        if size[0] < needed[0] || size[1] < needed[1] {
            return Err("Not enough room for another session");
        }
        let w = self.window_of(target).ok_or("Pane no longer exists")?;
        if !self.windows[&w].root.as_ref().is_some_and(|r| r.leaves().contains(&target)) {
            return Err("Dock this pane before splitting");
        }
        Ok(())
    }
    pub(crate) fn split(
        &mut self,
        target: PaneId,
        chat: i64,
        axis: SplitAxis,
        size: [f32; 2],
    ) -> Result<PaneId, &'static str> {
        self.can_split(target, axis, size)?;
        let w = self.window_of(target).ok_or("Pane no longer exists")?;
        if self.pane_for(chat).is_some() {
            return Err("Session is already open");
        }
        let p = self.alloc();
        let s = self.alloc();
        let layout = self.windows.get_mut(&w).ok_or("Pane no longer exists")?;
        layout.root.as_mut().ok_or("Pane no longer exists")?.insert(target, p, s, axis, false);
        self.panes.insert(p, chat);
        self.focus(p);
        Ok(p)
    }
    pub(crate) fn close(&mut self, pane: PaneId) -> Result<(), &'static str> {
        let w = self.window_of(pane).ok_or("Pane no longer exists")?;
        self.windows.get_mut(&w).ok_or("Pane no longer exists")?.remove(pane);
        self.panes.remove(&pane);
        Ok(())
    }
    pub(crate) fn float(&mut self, pane: PaneId, bounds: Rect) -> Result<(), &'static str> {
        let w = self.window_of(pane).ok_or("Pane no longer exists")?;
        let layout = self.windows.get_mut(&w).ok_or("Pane no longer exists")?;
        layout.remove(pane);
        layout.floats.push(FloatingPane { pane, bounds });
        layout.stack.push(pane);
        layout.focused = Some(pane);
        Ok(())
    }
    pub(crate) fn dock(
        &mut self,
        pane: PaneId,
        target: Option<PaneId>,
        side: DockSide,
    ) -> Result<(), &'static str> {
        let w = self.window_of(pane).ok_or("Pane no longer exists")?;
        let target = target.filter(|p| *p != pane);
        if let Some(t) = target
            && !self.windows[&w].root.as_ref().is_some_and(|r| r.leaves().contains(&t))
        {
            return Err("Dock target no longer exists");
        }
        let s = self.alloc();
        let layout = self.windows.get_mut(&w).ok_or("Pane no longer exists")?;
        layout.remove(pane);
        let before = matches!(side, DockSide::Left | DockSide::Top);
        let axis = if matches!(side, DockSide::Left | DockSide::Right) {
            SplitAxis::Horizontal
        } else {
            SplitAxis::Vertical
        };
        if let Some(root) = layout.root.as_mut() {
            let t = target.unwrap_or_else(|| root.leaves()[0]);
            root.insert(t, pane, s, axis, before);
        } else {
            layout.root = Some(DockNode::Leaf(pane));
        }
        layout.focused = Some(pane);
        Ok(())
    }
    pub(crate) fn transfer(&mut self, pane: PaneId, dest: WindowId) -> Result<(), &'static str> {
        let source = self.window_of(pane).ok_or("Pane no longer exists")?;
        if source == dest {
            return Ok(());
        }
        let s = self.alloc();
        self.windows.get_mut(&source).ok_or("Pane no longer exists")?.remove(pane);
        let target = self.windows.entry(dest).or_default();
        if let Some(root) = target.root.as_mut() {
            let leaves = root.leaves();
            let focused = target.focused.filter(|p| leaves.contains(p)).unwrap_or(leaves[0]);
            root.insert(focused, pane, s, SplitAxis::Horizontal, false);
        } else {
            target.root = Some(DockNode::Leaf(pane));
        }
        target.maximized = None;
        target.focused = Some(pane);
        Ok(())
    }
    pub(crate) fn forget_chat(&mut self, chat: i64) {
        if let Some(p) = self.pane_for(chat) {
            let _ = self.close(p);
        }
    }
    pub(crate) fn directional(
        &mut self,
        window: WindowId,
        direction: Direction,
        bounds: &BTreeMap<PaneId, Rect>,
        monocle: bool,
    ) -> Option<PaneId> {
        let layout = self.windows.get(&window)?;
        let candidates = if monocle {
            layout.root.as_ref().map(DockNode::leaves).unwrap_or_default()
        } else {
            layout.panes()
        };
        let candidate = neighbour(layout.focused?, candidates, direction, bounds, monocle)?;
        self.focus(candidate);
        Some(candidate)
    }
    pub(crate) fn swap_toward(
        &mut self,
        window: WindowId,
        direction: Direction,
        bounds: &BTreeMap<PaneId, Rect>,
        monocle: bool,
    ) {
        let Some(layout) = self.windows.get(&window) else { return };
        let (Some(current), Some(root)) = (layout.focused, &layout.root) else { return };
        if let Some(target) = neighbour(current, root.leaves(), direction, bounds, monocle) {
            self.swap(current, target);
        }
    }
    fn swap(&mut self, a: PaneId, b: PaneId) {
        if let Some(root) = self.window_of(a).and_then(|w| self.windows.get_mut(&w)?.root.as_mut())
            && [a, b].iter().all(|p| root.leaves().contains(p))
        {
            root.swap(a, b);
        }
    }
    pub(crate) fn promote(&mut self, pane: PaneId) {
        let first =
            self.window_of(pane).and_then(|w| Some(self.windows[&w].root.as_ref()?.leaves()[0]));
        if let Some(first) = first {
            self.swap(pane, first);
        }
    }
    // only on change, so divider drags stick until the next docked-pane change
    pub(crate) fn retile(&mut self, window: WindowId, layout: TileLayout, ratio: f32) {
        let Some(w) = self.windows.get_mut(&window) else { return };
        let leaves = w.root.as_ref().map(DockNode::leaves).unwrap_or_default();
        let key = Some((layout, ratio, leaves.clone()));
        if w.tiled == key {
            return;
        }
        w.tiled = key;
        if let Some(root) = self.tile(layout, &leaves, ratio) {
            self.windows.get_mut(&window).unwrap().root = Some(root);
        }
    }
    fn tile(&mut self, layout: TileLayout, leaves: &[PaneId], ratio: f32) -> Option<DockNode> {
        let leaf = |p: &PaneId| DockNode::Leaf(*p);
        match layout {
            TileLayout::Manual | TileLayout::Monocle => None,
            TileLayout::Columns => {
                self.chain(leaves.iter().map(leaf).collect(), SplitAxis::Horizontal)
            }
            TileLayout::Grid => {
                let cols = (leaves.len() as f32).sqrt().ceil().max(1.) as usize;
                let rows = leaves
                    .chunks(cols)
                    .filter_map(|row| {
                        self.chain(row.iter().map(leaf).collect(), SplitAxis::Horizontal)
                    })
                    .collect();
                self.chain(rows, SplitAxis::Vertical)
            }
            TileLayout::Stack => {
                let (main, rest) = leaves.split_first()?;
                let Some(stack) = self.chain(rest.iter().map(leaf).collect(), SplitAxis::Vertical)
                else {
                    return Some(leaf(main));
                };
                Some(DockNode::Split {
                    id: self.alloc(),
                    axis: SplitAxis::Horizontal,
                    ratio,
                    first: Box::new(leaf(main)),
                    second: Box::new(stack),
                })
            }
        }
    }
    fn chain(&mut self, nodes: Vec<DockNode>, axis: SplitAxis) -> Option<DockNode> {
        nodes.into_iter().rev().enumerate().fold(None, |rest, (i, node)| {
            Some(match rest {
                None => node,
                Some(rest) => DockNode::Split {
                    id: self.alloc(),
                    axis,
                    ratio: 1. / (i + 1) as f32,
                    first: Box::new(node),
                    second: Box::new(rest),
                },
            })
        })
    }
    pub(crate) fn cycle(&mut self, window: WindowId, back: bool) -> Option<PaneId> {
        let w = self.windows.get_mut(&window)?;
        let panes = w.panes();
        if panes.is_empty() {
            return None;
        }
        let i = w.focused.and_then(|p| panes.iter().position(|x| *x == p)).unwrap_or(0);
        let next = if back { (i + panes.len() - 1) % panes.len() } else { (i + 1) % panes.len() };
        let pane = panes[next];
        self.focus(pane);
        Some(pane)
    }
}

fn neighbour(
    current: PaneId,
    candidates: Vec<PaneId>,
    direction: Direction,
    bounds: &BTreeMap<PaneId, Rect>,
    tabs: bool,
) -> Option<PaneId> {
    if tabs {
        let i = candidates.iter().position(|p| *p == current)?;
        let j = match direction {
            Direction::Left | Direction::Up => i.checked_sub(1)?,
            Direction::Right | Direction::Down => i + 1,
        };
        return candidates.get(j).copied();
    }
    let center = |r: &Rect| [r.x + r.w * 0.5, r.y + r.h * 0.5];
    let origin = center(bounds.get(&current)?);
    candidates
        .into_iter()
        .filter(|p| *p != current)
        .filter_map(|p| {
            let target = center(bounds.get(&p)?);
            let dx = target[0] - origin[0];
            let dy = target[1] - origin[1];
            let allowed = match direction {
                Direction::Left => dx < 0.,
                Direction::Right => dx > 0.,
                Direction::Up => dy < 0.,
                Direction::Down => dy > 0.,
            };
            allowed.then_some((p, dx * dx + dy * dy))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

#[cfg(test)]
#[path = "../../tests/unit/session_workspace/model.rs"]
mod tests;
