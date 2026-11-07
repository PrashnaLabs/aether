use std::collections::{HashMap, HashSet};
use std::mem::{Discriminant, discriminant};
use std::ops::Range;

use aether_core::{
    Ask, Message, NoticeKind, NoticeState, Request, Role, Step, ToolDetail, ToolStatus,
};

use super::activity::{ActivityKind, activity_kind, activity_needs_attention};
use super::changes::applied_edit;
use super::live::short_ms;
use super::notice::compacted;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    User,
    /// A side turn the CLI started from a late steer: its first step, shown as a user bubble.
    SteerAsUser(usize),
    /// `message.text[range]`; never ends inside an open code fence.
    Text(Range<usize>),
    Thought(usize),
    Tool(usize),
    /// Two or more consecutive finished top-level Read rows.
    Reads(Vec<usize>),
    /// A Task row and every descendant row in this reply, flattened in order.
    Subagent {
        step: usize,
        children: Vec<usize>,
    },
    /// Consecutive activities of one kind, in their original order.
    Activity {
        kind: ActivityKind,
        steps: Vec<Block>,
    },
    Search(usize),
    Skill(usize),
    Notice(usize),
    /// A finished stretch of work between two runs of text, behind one "Worked for" row.
    Worked(Vec<Block>),
    /// Startup notices, until the reply has text or another step.
    Startup(Vec<usize>),
    Steer(usize),
    /// Index into `message.requests`.
    Request(usize),
    Fatal,
    Stopped,
    /// After a message the chat was handed off from: the "Context handoff" divider.
    Handoff,
    Fold,
    Changes,
    Live,
    Footer,
}

pub struct ReplyView<'a> {
    /// The reply is streaming (last message of a busy chat).
    pub live: bool,
    /// The user expanded the finished reply's timeline.
    pub open: bool,
    /// The user stopped the run that produced this reply.
    pub stopped: bool,
    /// Agent chat: a finished reply lists its edits in the changes card.
    pub agent: bool,
    /// Hook runs get rows; failed hooks show either way.
    pub show_hooks: bool,
    pub show_thinking: bool,
    /// Tool ids whose rows the user opened. They stay visible when the reply folds.
    pub kept: &'a dyn Fn(&str) -> bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptRow {
    pub message: usize,
    pub block: Block,
}

pub fn transcript_rows<'a>(
    messages: &[Message],
    view: &dyn Fn(usize) -> ReplyView<'a>,
) -> Vec<TranscriptRow> {
    messages
        .iter()
        .enumerate()
        .flat_map(|(message, m)| {
            let mut blocks = match m.role {
                Role::User => vec![Block::User],
                Role::Assistant => reply_blocks(m, &view(message)),
            };
            if m.handoff.is_some() {
                blocks.push(Block::Handoff);
            }
            blocks.into_iter().map(move |block| TranscriptRow { message, block })
        })
        .collect()
}

/// A reply that only compacted: its last compaction row, with no fold or footer around it.
/// Anything else in it is what a finished reply folds away regardless.
fn bare_compaction(m: &Message, view: &ReplyView) -> Option<usize> {
    let quiet = |s: &Step| match s {
        Step::Notice(n) => n.kind != NoticeKind::Wake && n.state != NoticeState::Failed,
        s => matches!(s, Step::Unknown),
    };
    let settled = !view.live && !view.stopped && !m.failed && m.error.is_none();
    (settled && m.text.is_empty() && m.requests.is_empty() && m.steps.iter().all(quiet))
        .then(|| m.steps.iter().rposition(|s| matches!(s, Step::Notice(n) if compacted(n))))
        .flatten()
}

pub fn reply_blocks(m: &Message, view: &ReplyView) -> Vec<Block> {
    if let Some(step) = bare_compaction(m, view) {
        return vec![Block::Notice(step)];
    }
    let roots = subagent_roots(m);
    let is_startup = |s: &Step| matches!(s, Step::Notice(n) if n.kind == NoticeKind::Startup);
    let startup: Vec<usize> = (0..m.steps.len()).filter(|&i| is_startup(&m.steps[i])).collect();
    let booting =
        m.text.is_empty() && m.steps.iter().all(|s| is_startup(s) || matches!(s, Step::Unknown));

    let holder = |req: &Request| {
        let Ask::Approval(a) = &req.ask else { return None };
        let id = a.tool_id.as_deref()?;
        let step = m.steps.iter().position(|s| matches!(s, Step::Tool(c) if c.id == id))?;
        Some(roots[step].unwrap_or(step))
    };
    let anchored: Vec<Option<usize>> = m.requests.iter().map(holder).collect();
    let mut items: Vec<(usize, bool, usize)> = (0..m.steps.len())
        .map(|i| (m.step_at(i), false, i))
        .chain(
            m.requests
                .iter()
                .enumerate()
                .filter(|&(r, _)| anchored[r].is_none())
                .map(|(r, req)| (req.at.min(m.text.len()), true, r)),
        )
        .collect();
    items.sort_by_key(|&(pos, request, _)| (pos, request));

    let mut body = Vec::new();
    let mut cursor = 0;
    for (pos, request, i) in items {
        let step = if request { None } else { Some(&m.steps[i]) };
        let block = match step {
            None => Block::Request(i),
            Some(_) if roots[i].is_some() => continue,
            // live background work sits in its side panel; an approval keeps its card here
            Some(Step::Tool(call))
                if call.status == ToolStatus::Background && !anchored.contains(&Some(i)) =>
            {
                continue;
            }
            Some(s) if !view.show_hooks && activity_kind(s) == Some(ActivityKind::Hooks) => {
                continue;
            }
            Some(Step::Thought(_) | Step::Thinking { .. }) if !view.show_thinking => continue,
            Some(Step::Thought(_) | Step::Thinking { .. }) => Block::Thought(i),
            Some(Step::Tool(call)) if matches!(call.detail, ToolDetail::Task { .. }) => {
                let children = (0..m.steps.len()).filter(|&c| roots[c] == Some(i)).collect();
                Block::Subagent { step: i, children }
            }
            Some(Step::Tool(_)) => Block::Tool(i),
            Some(Step::Search { .. }) => Block::Search(i),
            Some(Step::Skill(_)) => Block::Skill(i),
            Some(Step::Notice(_)) if booting && startup.first() == Some(&i) => {
                Block::Startup(startup.clone())
            }
            Some(Step::Notice(n))
                if n.kind == NoticeKind::Startup && (booting || n.state != NoticeState::Failed) =>
            {
                continue;
            }
            // the live row already says this while it runs
            Some(Step::Notice(n))
                if view.live
                    && n.kind == NoticeKind::Compaction
                    && n.state == NoticeState::Running =>
            {
                continue;
            }
            Some(Step::Notice(_)) => Block::Notice(i),
            Some(Step::Steer(_)) if i == 0 && pos == 0 => Block::SteerAsUser(i),
            Some(Step::Steer(_)) => Block::Steer(i),
            Some(Step::Unknown) => continue,
        };
        let split = safe_split(&m.text, pos);
        if split > cursor {
            body.push(Block::Text(cursor..split));
            cursor = split;
        }
        body.push(block);
        if !request {
            body.extend(
                (0..m.requests.len()).filter(|&r| anchored[r] == Some(i)).map(Block::Request),
            );
        }
    }
    if cursor < m.text.len() {
        body.push(Block::Text(cursor..m.text.len()));
    }

    let rows = || m.steps.iter().filter_map(|s| if let Step::Tool(c) = s { Some(c) } else { None });
    let mut tail = Vec::new();
    if m.error.is_some() {
        tail.push(Block::Fatal);
    }
    if rows().any(|c| c.status == ToolStatus::Stopped) || (view.stopped && !view.live) {
        tail.push(Block::Stopped);
    }
    if !view.live && view.agent && m.steps.iter().any(|s| applied_edit(s).is_some()) {
        tail.push(Block::Changes);
    }
    tail.push(if view.live { Block::Live } else { Block::Footer });

    let busy = m
        .steps
        .iter()
        .any(|s| matches!(s, Step::Notice(n) if n.state == NoticeState::Running))
        || rows().any(|c| {
            matches!(c.status, ToolStatus::Running | ToolStatus::Waiting | ToolStatus::Background)
        });
    let has_steps = body.iter().any(|b| !matches!(b, Block::Text(_) | Block::Request(_)));
    let done_read = |b: &Block| {
        matches!(b, Block::Tool(i) if matches!(&m.steps[*i], Step::Tool(c)
            if c.status == ToolStatus::Done && c.error.is_none() && matches!(c.detail, ToolDetail::Read { .. })))
    };
    let id = |i: usize| match &m.steps[i] {
        Step::Tool(c) => c.id.as_str(),
        _ => "",
    };
    body = group_activity(m, body);
    let mut blocks = Vec::new();
    if !view.live && !busy && has_steps {
        blocks.push(Block::Fold);
        if !view.open {
            // Lifecycle notices may arrive during or after the final answer.
            // Keep the final prose run, including text split by those notices,
            // instead of assuming the answer must follow the very last event.
            let last_text = body.iter().rposition(|b| matches!(b, Block::Text(_)));
            let answer_after = last_text.and_then(|end| {
                body[..end].iter().rposition(|b| {
                    !matches!(
                        b,
                        Block::Text(_)
                            | Block::Notice(_)
                            | Block::Startup(_)
                            | Block::Activity { kind: ActivityKind::Hooks, .. }
                    )
                })
            });
            let opened_groups: HashSet<usize> = body
                .chunk_by(|a, b| done_read(a) && done_read(b))
                .filter(|run| {
                    matches!(run, [Block::Tool(first), _, ..]
                        if (view.kept)(&read_group_id(id(*first))))
                })
                .flatten()
                .filter_map(|b| if let Block::Tool(i) = b { Some(*i) } else { None })
                .collect();
            body = body
                .into_iter()
                .enumerate()
                .filter(|(ix, b)| match b {
                    Block::Text(_) => Some(*ix) > answer_after,
                    Block::SteerAsUser(_) | Block::Request(_) => true,
                    Block::Activity { steps, .. } => {
                        (view.kept)(&activity_group_id(steps))
                            || steps
                                .iter()
                                .any(|b| matches!(b, Block::Tool(i) if (view.kept)(id(*i))))
                    }
                    Block::Notice(i) => {
                        // A failed startup may need user action before another run;
                        // a wake says why this reply exists at all;
                        // a compaction changes what the agent remembers.
                        matches!(&m.steps[*i], Step::Notice(n) if n.kind == NoticeKind::Wake
                            || compacted(n)
                            || (matches!(n.kind, NoticeKind::Startup | NoticeKind::Compaction)
                                && n.state == NoticeState::Failed))
                    }
                    Block::Subagent { step, children } => {
                        (view.kept)(id(*step))
                            || children.iter().any(|i| {
                                (view.kept)(id(*i))
                                    || (view.kept)(&format!("{}-activity-{}", id(*step), id(*i)))
                            })
                    }
                    Block::Tool(i) => opened_groups.contains(i) || (view.kept)(id(*i)),
                    _ => false,
                })
                .map(|(_, b)| b)
                .collect();
        }
    }
    let body: Vec<Block> = body
        .chunk_by(|a, b| done_read(a) && done_read(b))
        .map(|run| match run {
            [one] => one.clone(),
            _ => Block::Reads(
                run.iter()
                    .filter_map(|b| if let Block::Tool(i) = b { Some(*i) } else { None })
                    .collect(),
            ),
        })
        .collect();
    let folded = !view.live && !busy && has_steps && !view.open;
    blocks.extend(if folded { body } else { group_work(m, body, view.live) });
    blocks.extend(tail);
    blocks
}

/// Index of the source step for a leaf activity.
pub fn block_step(block: &Block) -> Option<usize> {
    match block {
        Block::Thought(i)
        | Block::Tool(i)
        | Block::Search(i)
        | Block::Skill(i)
        | Block::Notice(i) => Some(*i),
        _ => None,
    }
}

/// Every step index `block` shows, in order.
pub fn block_steps(block: &Block) -> Vec<usize> {
    match block {
        Block::Reads(reads) => reads.clone(),
        Block::Subagent { step, .. } => vec![*step],
        Block::Activity { steps, .. } | Block::Worked(steps) => {
            steps.iter().flat_map(block_steps).collect()
        }
        _ => block_step(block).into_iter().collect(),
    }
}

/// "Worked for 1m 20s", or "Thought for 4s" when it was all thinking.
pub fn work_label(m: &Message, blocks: &[Block]) -> String {
    let steps: Vec<&Step> =
        blocks.iter().flat_map(block_steps).filter_map(|i| m.steps.get(i)).collect();
    let verb = if steps.iter().all(|s| matches!(s, Step::Thought(_) | Step::Thinking { .. })) {
        "Thought"
    } else {
        "Worked"
    };
    let ms: u64 = steps
        .iter()
        .filter_map(|s| match s {
            Step::Tool(c) => c.ms,
            Step::Thinking { ms, .. } => *ms,
            _ => None,
        })
        .sum();
    if ms == 0 { verb.to_string() } else { format!("{verb} for {}", short_ms(ms)) }
}

/// Folds each run of two or more quiet blocks once text follows it, or once the reply ends.
/// Failures, approvals, steers and compactions stay out so they're never hidden.
fn group_work(m: &Message, blocks: Vec<Block>, live: bool) -> Vec<Block> {
    let stays_out =
        |s: &Step| activity_needs_attention(s) || matches!(s, Step::Notice(n) if compacted(n));
    let quiet = |b: &Block| {
        matches!(
            b,
            Block::Thought(_)
                | Block::Tool(_)
                | Block::Reads(_)
                | Block::Subagent { .. }
                | Block::Activity { .. }
                | Block::Search(_)
                | Block::Skill(_)
                | Block::Notice(_)
        ) && !block_steps(b).iter().any(|&i| m.steps.get(i).is_some_and(stays_out))
    };
    let running = |b: &Block| {
        block_steps(b).iter().any(
            |&i| matches!(m.steps.get(i), Some(Step::Tool(c)) if c.status == ToolStatus::Running),
        )
    };
    let (total, last_text) =
        (blocks.len(), blocks.iter().rposition(|b| matches!(b, Block::Text(_))));
    let mut out = Vec::new();
    let mut run = Vec::new();
    let flush = |run: &mut Vec<Block>, out: &mut Vec<Block>, end: usize| {
        let done = !live || (last_text.is_some_and(|t| t >= end) && !run.iter().any(running));
        if run.len() > 1 && done {
            out.push(Block::Worked(std::mem::take(run)));
        } else {
            out.append(run);
        }
    };
    for (ix, block) in blocks.into_iter().enumerate() {
        if quiet(&block) {
            run.push(block);
        } else {
            flush(&mut run, &mut out, ix);
            out.push(block);
        }
    }
    flush(&mut run, &mut out, total);
    // an opened reply with one stretch of work already sits behind its fold
    if !live && out.iter().filter(|b| matches!(b, Block::Worked(_))).count() == 1 {
        return out
            .into_iter()
            .flat_map(|b| match b {
                Block::Worked(steps) => steps,
                b => vec![b],
            })
            .collect();
    }
    out
}

pub fn activity_group_id(steps: &[Block]) -> String {
    format!("activity-{}", steps.first().and_then(block_step).unwrap_or_default())
}

fn group_activity(m: &Message, body: Vec<Block>) -> Vec<Block> {
    let kind = |b: &Block| {
        block_step(b).and_then(|i| activity_kind(&m.steps[i])).filter(|k| *k != ActivityKind::Reads)
    };
    body.chunk_by(|a, b| kind(a).is_some() && kind(a) == kind(b))
        .map(|run| match run {
            [one] => one.clone(),
            _ => Block::Activity { kind: kind(&run[0]).unwrap(), steps: run.to_vec() },
        })
        .collect()
}

fn subagent_roots(m: &Message) -> Vec<Option<usize>> {
    let tasks: HashMap<&str, usize> = m
        .steps
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Step::Tool(c) if matches!(c.detail, ToolDetail::Task { .. }) => {
                Some((c.id.as_str(), i))
            }
            _ => None,
        })
        .collect();
    let up = |i: usize| match &m.steps[i] {
        Step::Tool(c) => c.parent.as_deref().and_then(|p| tasks.get(p).copied()),
        _ => None,
    };
    (0..m.steps.len())
        .map(|i| {
            let mut top = up(i)?;
            // bounded: CLI-reported parents may form a cycle
            for _ in 0..m.steps.len() {
                match up(top) {
                    Some(next) => top = next,
                    None => return Some(top),
                }
            }
            None
        })
        .collect()
}

/// `at`, or the end of the closing fence line when `at` falls inside an open fence (text.len() if unclosed).
pub fn safe_split(text: &str, at: usize) -> usize {
    let mut fence: Option<(char, usize)> = None;
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        if fence.is_none() && start >= at {
            return at;
        }
        let end = start + line.len();
        let body = line.trim_start_matches(' ');
        match fence {
            None => {
                if let Some(c @ ('`' | '~')) = body.chars().next() {
                    let run = body.len() - body.trim_start_matches(c).len();
                    if run >= 3 {
                        fence = Some((c, run));
                    }
                }
            }
            Some((c, len)) => {
                let rest = body.trim_start_matches(c);
                if body.len() - rest.len() >= len && rest.trim().is_empty() {
                    if at < end {
                        return end;
                    }
                    fence = None;
                }
            }
        }
        start = end;
    }
    if fence.is_some() { text.len() } else { at }
}

/// What a row is, whatever it holds: a streaming text or a growing group keeps its place.
fn identity(row: &TranscriptRow) -> (usize, Discriminant<Block>, usize) {
    let start = match &row.block {
        Block::Text(range) => range.start,
        Block::Activity { steps, .. } => steps.first().and_then(block_step).unwrap_or_default(),
        Block::Worked(steps) => steps.iter().flat_map(block_steps).next().unwrap_or_default(),
        Block::Reads(steps) | Block::Startup(steps) => steps.first().copied().unwrap_or_default(),
        Block::SteerAsUser(i)
        | Block::Thought(i)
        | Block::Tool(i)
        | Block::Subagent { step: i, .. }
        | Block::Search(i)
        | Block::Skill(i)
        | Block::Notice(i)
        | Block::Steer(i)
        | Block::Request(i) => *i,
        _ => 0,
    };
    (row.message, discriminant(&row.block), start)
}

/// The list splices from `old` to `new`, ascending (old range, new count), one per run
/// between rows that kept their identity, and the survivors of `new` that changed what
/// they hold. Splicing only between survivors keeps a reader's scroll anchor on them.
pub fn row_splice(
    old: &[TranscriptRow],
    new: &[TranscriptRow],
) -> (Vec<(Range<usize>, usize)>, Vec<usize>) {
    let at: HashMap<_, usize> = new.iter().enumerate().map(|(j, r)| (identity(r), j)).collect();
    let mut survivors = Vec::new();
    for (i, row) in old.iter().enumerate() {
        let next = survivors.last().map_or(0, |&(_, j)| j + 1);
        if let Some(&j) = at.get(&identity(row)).filter(|&&j| j >= next) {
            survivors.push((i, j));
        }
    }
    let mut splices = Vec::new();
    let mut from = (0, 0);
    for &(i, j) in survivors.iter().chain([&(old.len(), new.len())]) {
        if i > from.0 || j > from.1 {
            splices.push((from.0..i, j - from.1));
        }
        from = (i + 1, j + 1);
    }
    let changed = survivors.into_iter().filter(|&(i, j)| old[i] != new[j]).map(|(_, j)| j);
    (splices, changed.collect())
}

/// What a "Read N files" group's open state is kept under: its first read's id, suffixed.
pub fn read_group_id(first: &str) -> String {
    format!("{first}-reads")
}

/// Top padding in px for `row`: 0 first, 32 between messages, 12 around text and cards, 4 between step rows.
pub fn gap_before(prev: Option<&TranscriptRow>, row: &TranscriptRow) -> f32 {
    let spaced = |b: &Block| {
        matches!(
            b,
            Block::Text(_)
                | Block::User
                | Block::SteerAsUser(_)
                | Block::Request(_)
                | Block::Fatal
                | Block::Stopped
                | Block::Changes
                | Block::Live
                | Block::Footer
        )
    };
    match prev {
        None => 0.,
        Some(p) if p.message != row.message => 32.,
        Some(p) if spaced(&p.block) || spaced(&row.block) => 12.,
        Some(_) => 4.,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/chat/timeline.rs"]
mod tests;
