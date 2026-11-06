use aether_agent::SavedCard;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Col {
    Backlog,
    Todo,
    Doing,
    Review,
    Done,
}

impl Col {
    pub(crate) const ALL: [Col; 5] = [Col::Backlog, Col::Todo, Col::Doing, Col::Review, Col::Done];

    pub(crate) fn key(self) -> &'static str {
        match self {
            Col::Backlog => "backlog",
            Col::Todo => "todo",
            Col::Doing => "doing",
            Col::Review => "review",
            Col::Done => "done",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Col::Backlog => "Backlog",
            Col::Todo => "Todo",
            Col::Doing => "Doing",
            Col::Review => "Review",
            Col::Done => "Done",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Col> {
        let s = s.trim().to_lowercase();
        Col::ALL.into_iter().find(|c| c.key() == s)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoardCard {
    pub id: i64,
    pub title: String,
    pub body: String,
    pub col: Col,
    pub position: f64,
    pub chat: Option<i64>,
    pub project: String,
    pub updated_at: i64,
}

impl From<SavedCard> for BoardCard {
    fn from(c: SavedCard) -> Self {
        BoardCard {
            id: c.id,
            title: c.title,
            body: c.body,
            col: Col::parse(&c.col).unwrap_or(Col::Todo),
            position: c.position,
            chat: c.chat,
            project: c.project,
            updated_at: c.updated_at,
        }
    }
}

impl From<&BoardCard> for SavedCard {
    fn from(c: &BoardCard) -> Self {
        SavedCard {
            id: c.id,
            title: c.title.clone(),
            body: c.body.clone(),
            col: c.col.key().to_string(),
            position: c.position,
            chat: c.chat,
            project: c.project.clone(),
            updated_at: c.updated_at,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Turn {
    Started,
    Finished { failed: bool },
}

pub(crate) fn auto_move(col: Col, turn: Turn) -> Option<Col> {
    match (col, turn) {
        (Col::Backlog | Col::Todo, Turn::Started) => Some(Col::Doing),
        (Col::Doing, Turn::Finished { failed: false }) => Some(Col::Review),
        _ => None,
    }
}

pub(crate) fn position_between(prev: Option<f64>, next: Option<f64>) -> f64 {
    match (prev, next) {
        (Some(p), Some(n)) => (p + n) / 2.0,
        (Some(p), None) => p + 1.0,
        (None, Some(n)) => n - 1.0,
        (None, None) => 1.0,
    }
}

pub(crate) fn list_line(card: &BoardCard) -> String {
    let mut s = format!("#{} [{}] {}", card.id, card.col.key(), card.title);
    if let Some(chat) = card.chat {
        s.push_str(&format!(" (agent {chat})"));
    }
    s
}

pub(crate) fn lead_move(card: &mut BoardCard, to: &str) -> Result<String, String> {
    let col = Col::parse(to).ok_or_else(|| {
        format!(r#"unknown column "{}"; use backlog, todo, doing or review"#, to.trim())
    })?;
    if col == Col::Done {
        return Err("only a human can move a card to done".into());
    }
    let from = card.col;
    card.col = col;
    Ok(format!("Moved #{} {} -> {}: {}", card.id, from.key(), col.key(), card.title))
}

fn sorted(cards: &[BoardCard], col: Col, skip: i64) -> Vec<&BoardCard> {
    let mut v: Vec<_> = cards.iter().filter(|c| c.col == col && c.id != skip).collect();
    v.sort_by(|a, b| a.position.total_cmp(&b.position));
    v
}

pub(crate) fn slot(cards: &[BoardCard], moved: i64, col: Col, before: Option<i64>) -> f64 {
    let list = sorted(cards, col, moved);
    let at = before.and_then(|b| list.iter().position(|c| c.id == b));
    match at {
        Some(i) => {
            position_between(i.checked_sub(1).map(|p| list[p].position), Some(list[i].position))
        }
        None => position_between(list.last().map(|c| c.position), None),
    }
}

pub(crate) fn open_lines(cards: &[BoardCard]) -> String {
    let lines: Vec<String> = Col::ALL
        .into_iter()
        .filter(|&c| c != Col::Done)
        .flat_map(|c| sorted(cards, c, -1))
        .map(list_line)
        .collect();
    if lines.is_empty() { "No open cards.".into() } else { lines.join("\n") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(col: Col, chat: Option<i64>) -> BoardCard {
        BoardCard {
            id: 7,
            title: "Fix parser".into(),
            body: String::new(),
            col,
            position: 1.0,
            chat,
            project: "p".into(),
            updated_at: 0,
        }
    }

    #[test]
    fn parse_keys() {
        assert_eq!(Col::parse(" Doing "), Some(Col::Doing));
        assert_eq!(Col::parse("blocked"), None);
        for c in Col::ALL {
            assert_eq!(Col::parse(c.key()), Some(c));
        }
    }

    #[test]
    fn auto_move_rules() {
        use Col::*;
        assert_eq!(auto_move(Backlog, Turn::Started), Some(Doing));
        assert_eq!(auto_move(Todo, Turn::Started), Some(Doing));
        assert_eq!(auto_move(Review, Turn::Started), None);
        assert_eq!(auto_move(Doing, Turn::Finished { failed: false }), Some(Review));
        assert_eq!(auto_move(Doing, Turn::Finished { failed: true }), None);
        assert_eq!(auto_move(Done, Turn::Started), None);
    }

    #[test]
    fn positions() {
        assert_eq!(position_between(None, None), 1.0);
        assert_eq!(position_between(Some(3.0), None), 4.0);
        assert_eq!(position_between(None, Some(2.0)), 1.0);
        assert_eq!(position_between(Some(1.0), Some(2.0)), 1.5);
        let mut next = 2.0;
        for _ in 0..40 {
            let p = position_between(Some(1.0), Some(next));
            assert!(1.0 < p && p < next);
            next = p;
        }
    }

    #[test]
    fn list_line_format() {
        assert_eq!(list_line(&card(Col::Doing, Some(42))), "#7 [doing] Fix parser (agent 42)");
        assert_eq!(list_line(&card(Col::Doing, None)), "#7 [doing] Fix parser");
    }

    #[test]
    fn lead_move_rules() {
        let mut c = card(Col::Todo, None);
        assert_eq!(lead_move(&mut c, "review"), Ok("Moved #7 todo -> review: Fix parser".into()));
        assert_eq!(c.col, Col::Review);
        let mut c = card(Col::Todo, None);
        assert_eq!(lead_move(&mut c, "done"), Err("only a human can move a card to done".into()));
        assert_eq!(c.col, Col::Todo);
        assert_eq!(
            lead_move(&mut c, "nope"),
            Err(r#"unknown column "nope"; use backlog, todo, doing or review"#.into())
        );
        assert_eq!(c.col, Col::Todo);
    }

    #[test]
    fn slot_positions() {
        let mut a = card(Col::Todo, None);
        a.id = 1;
        a.position = 1.0;
        let mut b = a.clone();
        b.id = 2;
        b.position = 3.0;
        let mut m = a.clone();
        m.id = 3;
        m.position = 9.0;
        let cards = [a, b, m];
        assert_eq!(slot(&cards, 3, Col::Todo, Some(2)), 2.0);
        assert_eq!(slot(&cards, 3, Col::Todo, Some(1)), 0.0);
        assert_eq!(slot(&cards, 3, Col::Todo, None), 4.0);
        assert_eq!(slot(&cards, 3, Col::Todo, Some(99)), 4.0);
        assert_eq!(slot(&cards, 3, Col::Review, None), 1.0);
    }

    #[test]
    fn open_lines_order() {
        assert_eq!(open_lines(&[]), "No open cards.");
        let mut a = card(Col::Doing, None);
        a.id = 1;
        let mut b = card(Col::Todo, Some(5));
        b.id = 2;
        let mut d = card(Col::Done, None);
        d.id = 3;
        assert_eq!(open_lines(&[a, b, d]), "#2 [todo] Fix parser (agent 5)\n#1 [doing] Fix parser");
    }
}
