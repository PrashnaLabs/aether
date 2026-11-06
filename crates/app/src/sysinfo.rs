use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub cpu_percent: f32,
    pub mem_used: u64,
    pub mem_total: u64,
}

// ponytail: fixed 3 s poll; make it a setting if anyone asks
pub const INTERVAL_SECS: u64 = 3;
pub const BACKOFF_SECS: u64 = 30;
pub const BACKOFF_AFTER: u32 = 3;

/// `fails` counts consecutive failures including this one.
pub fn next_sample(last: Option<Sample>, new: Option<Sample>, fails: u32) -> Option<Sample> {
    new.or(last.filter(|_| fails < BACKOFF_AFTER))
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_iostat_cpu(text: &str) -> Option<f32> {
    let idle: f32 = text.lines().last()?.split_whitespace().nth(2)?.parse().ok()?;
    Some((100. - idle).clamp(0., 100.))
}

pub fn parse_vm_stat(memsize: &str, vm_stat: &str) -> Option<(u64, u64)> {
    let total: u64 = memsize.trim().parse().ok()?;
    let page: u64 = vm_stat
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |key: &str| -> Option<u64> {
        vm_stat.lines().find_map(|l| l.strip_prefix(key))?.trim().trim_end_matches('.').parse().ok()
    };
    let app = pages("Anonymous pages:").or_else(|| pages("Pages active:"))?;
    let used = app.saturating_sub(pages("Pages purgeable:")?)
        + pages("Pages wired down:")?
        + pages("Pages occupied by compressor:")?;
    Some((used * page, total))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
/// (busy, total) jiffies from the aggregate `cpu` line.
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let f: Vec<u64> = text
        .lines()
        .next()?
        .strip_prefix("cpu ")?
        .split_whitespace()
        .map(|n| n.parse().ok())
        .collect::<Option<_>>()?;
    if f.len() < 5 {
        return None;
    }
    let total: u64 = f.iter().take(8).sum();
    Some((total - f[3] - f[4], total))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn cpu_delta(a: (u64, u64), b: (u64, u64)) -> Option<f32> {
    let total = b.1.checked_sub(a.1).filter(|t| *t > 0)?;
    Some((b.0.saturating_sub(a.0) as f32 / total as f32 * 100.).clamp(0., 100.))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let kb = |key: &str| -> Option<u64> {
        text.lines()
            .find_map(|l| l.strip_prefix(key))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    let total = kb("MemTotal:")?;
    Some((total.checked_sub(kb("MemAvailable:")?)? * 1024, total * 1024))
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "macos")]
pub fn sample() -> Option<Sample> {
    let cpu_percent =
        parse_iostat_cpu(&run("/usr/sbin/iostat", &["-n", "0", "-c", "2", "-w", "1"])?)?;
    let (mem_used, mem_total) = parse_vm_stat(
        &run("/usr/sbin/sysctl", &["-n", "hw.memsize"])?,
        &run("/usr/bin/vm_stat", &[])?,
    )?;
    Some(Sample { cpu_percent, mem_used, mem_total })
}

#[cfg(target_os = "linux")]
pub fn sample() -> Option<Sample> {
    let read = || parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?);
    let first = read()?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    let cpu_percent = cpu_delta(first, read()?)?;
    let (mem_used, mem_total) = parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)?;
    Some(Sample { cpu_percent, mem_used, mem_total })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn sample() -> Option<Sample> {
    None
}

#[cfg(test)]
#[path = "../tests/unit/sysinfo.rs"]
mod tests;
