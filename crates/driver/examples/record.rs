use std::path::PathBuf;
use std::time::{Duration, Instant};

use aether_core::{AccessMode, Answer, Approval, Ask, FieldKind, Update, form_answer};
use aether_driver::{
    AntigravityDriver, ClaudeDriver, CodexDriver, Driver, DriverStartOptions, OpencodeDriver,
    PiDriver,
};
use futures::channel::mpsc::{TryRecvError, unbounded};

const USAGE: &str = "usage: record --provider claude|codex|opencode|pi|agy --prompt TEXT [flags]
  --provider P            which Driver to spawn
  --cwd DIR               the project folder (default .)
  --prompt TEXT           the first prompt
  --then TEXT             a second prompt, sent after the first Final (repeatable)
  --access M              ask|read-only|auto-edit|auto|full (default ask)
  --model ID, --effort E, --resume ID
  --answer A              allow|deny|always|first|note:TEXT (default allow)
  --steer-after MS --steer TEXT   steer once, MS ms after the first prompt
  --cancel-after MS       cancel once, MS ms after the first prompt
  --linger SECS           keep printing this long after the last Final
  --timeout SECS          give up (default 240)
set AETHER_RECORD=DIR to write the scrubbed stream to DIR/<provider>-<ms>.jsonl";

#[derive(Clone, PartialEq)]
enum Policy {
    Allow,
    Deny,
    Always,
    First,
    Note(String),
}

struct Args {
    provider: String,
    opts: DriverStartOptions,
    prompts: Vec<String>,
    policy: Policy,
    steer: Option<(u64, String)>,
    cancel_after: Option<u64>,
    linger: u64,
    timeout: u64,
}

fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut opts = DriverStartOptions {
        binary: String::new(),
        cwd: PathBuf::from("."),
        model: String::new(),
        effort: String::new(),
        access: AccessMode::Ask,
        resume: String::new(),
        reattach: false,
        attachments: None,
        background: Vec::new(),
        lead: None,
    };
    let (mut provider, mut prompts, mut policy) = (String::new(), vec![], Policy::Allow);
    let (mut steer_after, mut steer, mut cancel_after) = (None, None, None);
    let (mut linger, mut timeout) = (0, 240);
    let mut it = argv.into_iter();
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        let num = |v: String| v.parse::<u64>().map_err(|_| format!("{flag}: not a number: {v}"));
        match flag.as_str() {
            "--provider" => provider = val()?,
            "--cwd" => opts.cwd = PathBuf::from(val()?),
            "--prompt" => prompts.insert(0, val()?),
            "--then" => prompts.push(val()?),
            "--model" => opts.model = val()?,
            "--effort" => opts.effort = val()?,
            "--resume" => opts.resume = val()?,
            "--access" => {
                opts.access = match val()?.as_str() {
                    "ask" => AccessMode::Ask,
                    "read-only" => AccessMode::ReadOnly,
                    "auto-edit" => AccessMode::AutoEdit,
                    "auto" => AccessMode::Auto,
                    "full" => AccessMode::Full,
                    other => return Err(format!("unknown access: {other}")),
                }
            }
            "--answer" => {
                policy = match val()?.as_str() {
                    "allow" => Policy::Allow,
                    "deny" => Policy::Deny,
                    "always" => Policy::Always,
                    "first" => Policy::First,
                    note if note.starts_with("note:") => Policy::Note(note[5..].to_string()),
                    other => return Err(format!("unknown answer: {other}")),
                }
            }
            "--steer-after" => steer_after = Some(num(val()?)?),
            "--steer" => steer = Some(val()?),
            "--cancel-after" => cancel_after = Some(num(val()?)?),
            "--linger" => linger = num(val()?)?,
            "--timeout" => timeout = num(val()?)?,
            _ => return Err(format!("unknown flag: {flag}")),
        }
    }
    if prompts.is_empty() || provider.is_empty() {
        return Err("--provider and --prompt are required".into());
    }
    let steer = match (steer_after, steer) {
        (Some(ms), Some(text)) => Some((ms, text)),
        (None, None) => None,
        _ => return Err("--steer-after and --steer go together".into()),
    };
    Ok(Args { provider, opts, prompts, policy, steer, cancel_after, linger, timeout })
}

fn answer(policy: &Policy, ask: &Ask) -> Answer {
    match (policy, ask) {
        (Policy::Deny, _) => Answer::Deny,
        (Policy::Note(note), _) => Answer::DenyWith(note.clone()),
        (Policy::Always, Ask::Approval(Approval { options, .. })) if !options.is_empty() => {
            Answer::Pick(options[0].id.clone())
        }
        (_, Ask::Approval(Approval { input, .. })) => {
            Answer::Approve { input: input.clone(), always: *policy == Policy::Always }
        }
        (_, Ask::Question { choices, .. }) => {
            Answer::Reply(choices.first().cloned().unwrap_or_default())
        }
        (_, Ask::Questions(questions)) => Answer::Answers(
            questions
                .iter()
                .map(|q| q.options.iter().take(1).map(|c| c.label.clone()).collect())
                .collect(),
        ),
        (_, Ask::Form(form)) => {
            let values: Vec<Vec<String>> = form
                .fields
                .iter()
                .map(|f| {
                    let first = f
                        .options
                        .first()
                        .map(|c| if c.value.is_empty() { &c.label } else { &c.value });
                    match (first, f.kind) {
                        (Some(v), _) => vec![v.clone()],
                        (None, FieldKind::Bool) => vec!["true".into()],
                        (None, _) => f.default.clone(),
                    }
                })
                .collect();
            form_answer(form, &values).unwrap_or(Answer::Deny)
        }
        (_, Ask::Plan(_)) => Answer::Plan { approved: true, text: String::new() },
        (_, Ask::Unknown) => Answer::Deny,
    }
}

fn main() {
    let args = match parse(std::env::args().skip(1).collect()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            std::process::exit(1);
        }
    };
    let (tx, mut rx) = unbounded();
    let opts = args.opts.clone();
    let mut driver: Box<dyn Driver> = match args.provider.as_str() {
        "claude" => Box::new(ClaudeDriver::spawn(opts, tx)),
        "codex" => Box::new(CodexDriver::spawn(opts, tx)),
        "opencode" => Box::new(OpencodeDriver::spawn(opts, tx)),
        "pi" => Box::new(PiDriver::spawn(opts, tx)),
        "agy" => Box::new(AntigravityDriver::spawn(opts, tx)),
        other => {
            eprintln!("unknown provider: {other}\n{USAGE}");
            std::process::exit(1);
        }
    };
    let start = Instant::now();
    let deadline = start + Duration::from_secs(args.timeout);
    let mut prompts = args.prompts.into_iter();
    driver.prompt(prompts.next().unwrap_or_default(), vec![]);
    let mut steer = args.steer;
    let mut cancel_after = args.cancel_after;
    let mut done: Option<Instant> = None;
    loop {
        let ms = start.elapsed().as_millis() as u64;
        if let Some((at, _)) = &steer
            && ms >= *at
        {
            let (_, text) = steer.take().unwrap();
            driver.steer(text, vec![]);
        }
        if cancel_after.is_some_and(|at| ms >= at) {
            cancel_after = None;
            driver.cancel();
        }
        let now = Instant::now();
        if now >= deadline {
            eprintln!("timeout");
            drop(driver);
            std::process::exit(2);
        }
        if done.is_some_and(|d| now >= d) {
            return;
        }
        let update = match rx.try_recv() {
            Ok(update) => update,
            Err(TryRecvError::Closed) => return,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(20));
                continue;
            }
        };
        println!("{ms:>6}ms {update:?}");
        match update {
            Update::Ask { id, ask } => driver.answer(id, answer(&args.policy, &ask)),
            Update::Final(_) => match prompts.next() {
                Some(next) => driver.prompt(next, vec![]),
                None => done = Some(Instant::now() + Duration::from_secs(args.linger)),
            },
            _ => {}
        }
    }
}
