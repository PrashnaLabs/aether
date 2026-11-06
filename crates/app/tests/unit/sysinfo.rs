use super::*;

// /usr/sbin/iostat -n 0 -c 2 -w 1, captured 2026-11-05
const IOSTAT: &str = "      cpu    load average\n us sy id   1m   5m   15m\n 46 13 41  4.06 4.74 5.63\n 34  9 56  4.06 4.74 5.63\n";

// /usr/bin/vm_stat, captured 2026-11-05
const VM_STAT: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                                    12188.\nPages active:                                 274233.\nPages inactive:                               267289.\nPages speculative:                              6556.\nPages throttled:                                   0.\nPages wired down:                             137577.\nPages purgeable:                                2293.\n\"Translation faults\":                      487269834.\nPages copy-on-write:                        26081277.\nPages zero filled:                         214451640.\nPages reactivated:                          53055068.\nPages purged:                                2566110.\nFile-backed pages:                            205878.\nAnonymous pages:                              342200.\nPages stored in compressor:                   600498.\nPages occupied by compressor:                 315062.\n";

#[test]
fn parses_iostat_last_interval() {
    assert_eq!(parse_iostat_cpu(IOSTAT), Some(44.));
}

#[test]
fn parses_vm_stat_and_memsize() {
    let (used, total) = parse_vm_stat("17179869184\n", VM_STAT).unwrap();
    assert_eq!(used, (342200 - 2293 + 137577 + 315062) * 16384);
    assert_eq!(total, 17179869184);
}

#[test]
fn vm_stat_without_anonymous_falls_back_to_active() {
    let old: String =
        VM_STAT.lines().filter(|l| !l.starts_with("Anonymous")).map(|l| format!("{l}\n")).collect();
    let (used, _) = parse_vm_stat("17179869184", &old).unwrap();
    assert_eq!(used, (274233 - 2293 + 137577 + 315062) * 16384);
}

#[test]
fn keeps_last_sample_until_backoff() {
    let last = Some(Sample { cpu_percent: 10., mem_used: 1, mem_total: 2 });
    let new = Some(Sample { cpu_percent: 20., mem_used: 1, mem_total: 2 });
    assert_eq!(next_sample(last, new, 0), new);
    for fails in 1..BACKOFF_AFTER {
        assert_eq!(next_sample(last, None, fails), last, "{fails}");
    }
    assert_eq!(next_sample(last, None, BACKOFF_AFTER), None);
}

#[test]
fn parses_proc_stat_delta() {
    let a = "cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 1 2 3 4\n";
    let b = "cpu  150 0 150 900 0 0 0 0 0 0\n";
    let cpu = cpu_delta(parse_proc_stat(a).unwrap(), parse_proc_stat(b).unwrap()).unwrap();
    assert!((cpu - 50.).abs() < 0.01, "{cpu}");
    assert_eq!(cpu_delta(parse_proc_stat(a).unwrap(), parse_proc_stat(a).unwrap()), None);
}

#[test]
fn parses_proc_meminfo() {
    let text =
        "MemTotal:       16000000 kB\nMemFree:         100000 kB\nMemAvailable:    6000000 kB\n";
    assert_eq!(parse_meminfo(text), Some((10_000_000 * 1024, 16_000_000 * 1024)));
}

#[test]
fn bad_input_gives_none() {
    assert_eq!(parse_iostat_cpu("nothing"), None);
    assert_eq!(parse_iostat_cpu(" us sy id   1m   5m   15m\n"), None);
    assert_eq!(parse_vm_stat("abc", VM_STAT), None);
    assert_eq!(parse_vm_stat("1", "Pages active: 1."), None);
    assert_eq!(parse_proc_stat("cpu  1 2"), None);
    assert_eq!(parse_meminfo("MemTotal: 1 kB"), None);
}
