//! CLI: homeostat snapshot | watch | demo.
//!
//!     homeostat snapshot --kanban /tmp/kanban.json --diagnostics /tmp/diag.json \
//!                        --team data/team --tupleboard /tmp/stats.json --window 3600
//!     homeostat watch --every 60 --kanban /tmp/kanban.json --diagnostics /tmp/diag.json
//!     homeostat demo
//!
//! Код выхода: 0 — всё живо (или только замечания), 1 — деградация, 2 — критично.
//! Это даёт прибору голос в cron и в CI.

use homeostat::sources;
use homeostat::{now, report, Facts, Report, Severity, System};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("snapshot") => cmd_snapshot(&args[1..]),
        Some("watch") => cmd_watch(&args[1..]),
        Some("demo") => cmd_demo(),
        Some("--help") | Some("-h") | None => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("неизвестная команда: {other}\n");
            print_help();
            2
        }
    };
    std::process::exit(code);
}

fn print_help() {
    println!(
        "homeostat — прибор живучести команды (VSM Бира, закон Эшби, POSIWID)

  snapshot [--kanban PATH] [--diagnostics PATH] [--team DIR]
           [--tupleboard PATH] [--outlook PATH] [--window S] [--json]
  watch    --every S [те же входы]
  demo

Входы не запускаются и не меняются — только читаются:
  kanban       вывод `hermes kanban list --json`
  diagnostics  вывод `hermes kanban diagnostics --json`
  team         каталог журналов tupleboard (closed/escalations/algedonic/policy)
  tupleboard   GET /stats, сохранённый в файл
  outlook      jsonl внешних изменений (например, события сканера архива)

Код выхода: 0 живо, 1 деградация, 2 критично."
    );
}

// --- разбор аргументов ---------------------------------------------------------
struct Opts {
    flags: Vec<(String, Option<String>)>,
}

impl Opts {
    fn parse(args: &[String]) -> Self {
        let mut flags = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if let Some(name) = arg.strip_prefix("--") {
                let (name, inline) = match name.split_once('=') {
                    Some((name, value)) => (name.to_string(), Some(value.to_string())),
                    None => (name.to_string(), None),
                };
                if inline.is_some() {
                    flags.push((name, inline));
                } else if index + 1 < args.len() && !args[index + 1].starts_with("--") {
                    flags.push((name, Some(args[index + 1].clone())));
                    index += 1;
                } else {
                    flags.push((name, None));
                }
            }
            index += 1;
        }
        Self { flags }
    }

    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .and_then(|(_, value)| value.as_deref())
    }

    fn number(&self, name: &str) -> Option<u64> {
        self.flag(name).and_then(|value| value.parse().ok())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(key, _)| key == name)
    }
}

// --- сборка снимка --------------------------------------------------------------
fn collect(opts: &Opts) -> (Facts, Vec<String>) {
    let mut facts = Facts::default();
    let mut not_connected: Vec<String> = Vec::new();
    let window = opts.number("window").unwrap_or(3600);

    let mut connect =
        |name: &str, value: Option<&str>, loader: &mut dyn FnMut(&PathBuf)| match value {
            Some(path) => {
                loader(&PathBuf::from(path));
            }
            None => not_connected.push(name.to_string()),
        };

    connect("kanban", opts.flag("kanban"), &mut |path| {
        sources::load_kanban(path, &mut facts);
    });
    connect("diagnostics", opts.flag("diagnostics"), &mut |path| {
        sources::load_diagnostics(path, &mut facts);
    });
    connect("team", opts.flag("team"), &mut |path| {
        sources::load_team(path, window, &mut facts);
    });
    connect("tupleboard", opts.flag("tupleboard"), &mut |path| {
        sources::load_tupleboard(path, &mut facts);
    });
    connect("outlook", opts.flag("outlook"), &mut |path| {
        sources::load_outlook(path, window, &mut facts);
    });

    (facts, not_connected)
}

fn build(opts: &Opts) -> (Report, Vec<String>) {
    let window = opts.number("window").unwrap_or(3600);
    let (facts, not_connected) = collect(opts);
    let built = report(facts, now(), window);
    (built, not_connected)
}

fn print_human(built: &Report, not_connected: &[String]) {
    println!(
        "вердикт: {} (код {}), окно {}с, снято {} (unix)",
        built.verdict.as_str(),
        built.verdict.code(),
        built.window,
        built.taken_at
    );
    let gap_word = if built.variety.gap < 0 {
        format!("не хватает {}", -built.variety.gap)
    } else {
        format!("запас {}", built.variety.gap)
    };
    println!(
        "разнообразие: требует внимания {}, переваривает {} — {}",
        built.variety.demand, built.variety.capacity, gap_word
    );
    if built.diagnoses.is_empty() {
        println!("\nконтуры подают признаки жизни: жалоб нет.");
    } else {
        println!("\n{:<6} {:<9} {}", "контур", "степень", "диагноз");
        for item in &built.diagnoses {
            println!(
                "{:<6} {:<9} {}",
                item.system.as_str(),
                item.severity.as_str(),
                item.text
            );
            for line in &item.evidence {
                println!("       · {line}");
            }
            println!("       → {}", item.advice);
        }
    }
    println!("\nвходы:");
    for state in &built.facts.inputs {
        println!(
            "  {:<12} {:<8} {} — {}",
            state.name,
            if state.loaded { "ok" } else { "НЕТ" },
            state.source,
            state.note
        );
    }
    for name in not_connected {
        println!("  {name:<12} {:<8} не подключён", "—");
    }
    let audit = built
        .facts
        .inputs
        .iter()
        .filter(|state| !state.loaded)
        .count();
    if audit > 0 {
        println!(
            "\nнепрочитанных входов: {audit} — это видно в диагнозе INPUT_MISSING, а не спрятано в нулях"
        );
    }
    if let Some(worst) = built
        .diagnoses
        .iter()
        .find(|item| item.system == System::S3Star && item.severity == Severity::Critical)
    {
        println!(
            "\nPOSIWID: система делает то, что показывает аудит. {}",
            worst.text
        );
    }
}

fn cmd_snapshot(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let (built, not_connected) = build(&opts);
    if opts.has("json") {
        match serde_json::to_string_pretty(&built) {
            Ok(text) => println!("{text}"),
            Err(err) => {
                eprintln!("не удалось сериализовать отчёт: {err}");
                return 2;
            }
        }
    } else {
        print_human(&built, &not_connected);
    }
    built.verdict.code()
}

fn cmd_watch(args: &[String]) -> i32 {
    let opts = Opts::parse(args);
    let every = opts.number("every").unwrap_or(60).max(1);
    loop {
        let (built, _) = build(&opts);
        let first = built
            .diagnoses
            .iter()
            .map(|item| item.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{} {} разнообразие {}/{} {}",
            built.taken_at,
            built.verdict.as_str(),
            built.variety.capacity,
            built.variety.demand,
            if first.is_empty() { "тихо" } else { &first }
        );
        sleep(Duration::from_secs(every));
    }
}

// --- демонстрация ---------------------------------------------------------------
fn cmd_demo() -> i32 {
    let mut checks: Vec<(String, bool)> = Vec::new();
    let check = |checks: &mut Vec<(String, bool)>, label: &str, ok: bool| {
        checks.push((label.to_string(), ok));
        println!("  {} {label}", if ok { "✓" } else { "✗" });
    };

    let healthy = Facts {
        running: 3,
        assignees_working: 2,
        in_flight: 1,
        ready: 0,
        stranded: 0,
        blocked: 0,
        review: 1,
        crashed_runs: 0,
        in_space: 2,
        refusals: 0,
        closed: 10,
        confirmed: 10,
        mismatched: 0,
        unverifiable: 0,
        environmental: 4,
        escalations: 0,
        algedonic: 0,
        algedonic_limit: 3,
        inputs: vec![],
    };
    let mut open = healthy.clone();
    open.running = 0;
    open.in_flight = 0;
    open.ready = 5;
    open.stranded = 3;
    open.closed = 4;
    open.confirmed = 1;
    open.mismatched = 3;
    open.environmental = 0;
    let mut overloaded = healthy.clone();
    overloaded.running = 1;
    overloaded.in_flight = 0;
    overloaded.confirmed = 0;
    overloaded.closed = 4;
    overloaded.ready = 12;
    overloaded.stranded = 0; // не залипание, а именно перегрузка объёмом
    overloaded.unverifiable = 3;
    overloaded.escalations = 4;
    overloaded.algedonic = 3;
    overloaded.algedonic_limit = 3;

    let scenarios = [
        ("здоровая команда", healthy, Severity::Ok),
        ("разомкнутые контуры", open, Severity::Critical),
        ("перегрузка разнообразием", overloaded, Severity::Degraded),
    ];

    for (name, facts, expected) in scenarios {
        println!("\n=== {name} ===");
        let built = report(facts, now(), 3600);
        print_human(&built, &[]);
        check(
            &mut checks,
            &format!("вердикт «{}»", expected.as_str()),
            built.verdict == expected,
        );
    }

    println!("\nитог проверок:");
    let failed = checks.iter().filter(|(_, ok)| !ok).count();
    println!(
        "  {} проверок, провалено {} — {}",
        checks.len(),
        failed,
        if failed == 0 {
            "прибор различает три состояния"
        } else {
            "НЕ работает"
        }
    );
    if failed == 0 {
        0
    } else {
        1
    }
}
