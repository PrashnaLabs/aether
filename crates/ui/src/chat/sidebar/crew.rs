use super::ChatStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bucket {
    NeedsYou,
    Failed,
    Working,
    Done,
}

impl Bucket {
    pub fn of(status: ChatStatus) -> Bucket {
        match status {
            ChatStatus::Waiting => Bucket::NeedsYou,
            ChatStatus::Failed => Bucket::Failed,
            ChatStatus::Starting
            | ChatStatus::Working { .. }
            | ChatStatus::Retrying
            | ChatStatus::Quiet { .. }
            | ChatStatus::Background(_) => Bucket::Working,
            ChatStatus::Done | ChatStatus::Idle => Bucket::Done,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Bucket::NeedsYou => "needs you",
            Bucket::Failed => "failed",
            Bucket::Working => "working",
            Bucket::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Narrow,
    Mid,
    Wide,
}

impl Tier {
    pub fn of(width: f32) -> Tier {
        if width < 235. {
            Tier::Narrow
        } else if width < 380. {
            Tier::Mid
        } else {
            Tier::Wide
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct CrewSummary {
    pub needs_you: usize,
    pub failed: usize,
    pub working: usize,
    pub done: usize,
}

impl CrewSummary {
    pub fn add(&mut self, status: ChatStatus) {
        match Bucket::of(status) {
            Bucket::NeedsYou => self.needs_you += 1,
            Bucket::Failed => self.failed += 1,
            Bucket::Working => self.working += 1,
            Bucket::Done => self.done += 1,
        }
    }

    pub fn total(&self) -> usize {
        self.needs_you + self.failed + self.working + self.done
    }

    pub fn worst(&self) -> Option<Bucket> {
        [
            (self.needs_you, Bucket::NeedsYou),
            (self.failed, Bucket::Failed),
            (self.working, Bucket::Working),
            (self.done, Bucket::Done),
        ]
        .into_iter()
        .find(|(n, _)| *n > 0)
        .map(|(_, b)| b)
    }

    /// Non-empty buckets the tier shows; below Wide only busy ones, unless all are done.
    pub fn parts(&self, tier: Tier) -> Vec<(Bucket, usize)> {
        let all = [
            (Bucket::NeedsYou, self.needs_you),
            (Bucket::Failed, self.failed),
            (Bucket::Working, self.working),
            (Bucket::Done, self.done),
        ];
        let busy: Vec<_> = all[..3].iter().copied().filter(|(_, n)| *n > 0).collect();
        if tier != Tier::Wide && !busy.is_empty() {
            return busy;
        }
        all.into_iter().filter(|(_, n)| *n > 0).collect()
    }

    pub fn words(&self, tier: Tier) -> String {
        self.parts(tier)
            .iter()
            .map(|(b, n)| format!("{n} {}", b.word()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(super) struct CrewMember {
    pub ix: usize,
    pub status: ChatStatus,
    pub activity: i64,
    pub keep: bool,
    pub summary: Option<CrewSummary>,
}

impl CrewMember {
    fn bucket(&self) -> Bucket {
        self.summary.and_then(|s| s.worst()).unwrap_or_else(|| Bucket::of(self.status))
    }
}

pub(super) fn order(members: &mut [CrewMember]) {
    members.sort_by_key(|m| (m.bucket(), std::cmp::Reverse(m.activity)));
}

pub(super) struct Preview {
    pub visible: Vec<bool>,
    pub hidden: CrewSummary,
}

pub(super) fn preview(members: &[CrewMember], expanded: bool, show_all: bool) -> Preview {
    let mut visible: Vec<bool> = members
        .iter()
        .map(|m| show_all || m.keep || (expanded && m.bucket() <= Bucket::Failed))
        .collect();
    if expanded && !show_all {
        let mut rest: Vec<usize> = (0..members.len()).filter(|&i| !visible[i]).collect();
        rest.sort_by_key(|&i| (members[i].bucket(), std::cmp::Reverse(members[i].activity)));
        for i in rest.into_iter().take(5) {
            visible[i] = true;
        }
    }
    let mut hidden = CrewSummary::default();
    for (m, _) in members.iter().zip(&visible).filter(|(_, v)| !**v) {
        hidden.add(m.status);
    }
    Preview { visible, hidden }
}

pub(super) fn starts_collapsed(children: usize) -> bool {
    children > 5
}

#[cfg(test)]
#[path = "../../../tests/unit/chat/sidebar/crew.rs"]
mod tests;
