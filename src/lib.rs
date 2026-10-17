//! homeostat — прибор живучести команды.
//!
//! Не доска, не очередь и не планировщик: homeostat ничего не выполняет и не хранит работу.
//! Он читает следы пяти контуров (по Бира: S1 операции, S2 согласование, S3 управление,
//! S3* аудит, S4 разведка среды, S5 политика) и говорит, **какой контур разомкнут**.
//!
//! Название — от гомеостата Эшби (1948): машина, которая перестраивала связи, чтобы удержать
//! равновесие. Здесь то же самое, но предмет — организация работы агентов.
//!
//! Панель живёт в [`dashboard`]: одна страница, которая пересобирает диагноз на каждый запрос.
//!
//! Три правила, на которых всё держится:
//! 1. прибор ничего не пишет в чужие системы — только читает;
//! 2. отсутствие сигнала — это диагноз «контур разомкнут», а не ноль;
//! 3. каждый вывод несёт числа, на которых он построен (иначе это гадание).
//!
//! Ограничение, которое надо помнить: «разнообразие» здесь — индикатор, а не закон.
//! Эшби сам оговаривал, что разнообразие зависит от наблюдателя и его различений,
//! поэтому demand/capacity — это выбранная нами шкала, а не физическая величина.

use serde::{Deserialize, Serialize};

pub mod dashboard;
pub mod sources;

/// Порог «карточка залипла»: те же 30 минут, что в диагностике канбана.
pub const STRANDED_AFTER: u64 = 1800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Note,
    Degraded,
    Critical,
}

impl Severity {
    pub fn rank(self) -> u8 {
        match self {
            Severity::Ok => 0,
            Severity::Note => 1,
            Severity::Degraded => 2,
            Severity::Critical => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Ok => "ok",
            Severity::Note => "note",
            Severity::Degraded => "degraded",
            Severity::Critical => "critical",
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Severity::Ok | Severity::Note => 0,
            Severity::Degraded => 1,
            Severity::Critical => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum System {
    S1,
    S2,
    S3,
    #[serde(rename = "S3*")]
    S3Star,
    S4,
    S5,
    #[serde(rename = "VSM")]
    Whole,
}

impl System {
    pub fn as_str(self) -> &'static str {
        match self {
            System::S1 => "S1",
            System::S2 => "S2",
            System::S3 => "S3",
            System::S3Star => "S3*",
            System::S4 => "S4",
            System::S5 => "S5",
            System::Whole => "VSM",
        }
    }
}

/// Что прибор прочитал и чего не нашёл. Пропавший вход — это диагноз, а не ноль.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputState {
    pub name: String,
    pub source: String,
    pub loaded: bool,
    pub note: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Facts {
    // S1 операции
    pub running: usize,
    pub assignees_working: usize,
    pub in_flight: usize,
    // S2 согласование
    pub ready: usize,
    pub stranded: usize,
    pub blocked: usize,
    pub review: usize,
    // S3 управление
    pub crashed_runs: usize,
    pub in_space: usize,
    pub refusals: u64,
    // S3* аудит
    pub closed: usize,
    pub confirmed: usize,
    pub mismatched: usize,
    pub unverifiable: usize,
    // S4 среда
    pub environmental: usize,
    // S5 политика
    pub escalations: usize,
    pub algedonic: usize,
    pub algedonic_limit: usize,
    // служебное
    pub inputs: Vec<InputState>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Variety {
    /// Сколько единиц требует внимания прямо сейчас.
    pub demand: i64,
    /// Сколько команда реально пропускает через контуры за окно.
    pub capacity: i64,
    /// capacity - demand. Меньше нуля — разнообразие не уравновешено.
    pub gap: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub code: String,
    pub system: System,
    pub severity: Severity,
    pub text: String,
    pub evidence: Vec<String>,
    pub advice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub taken_at: u64,
    pub window: u64,
    pub verdict: Severity,
    pub variety: Variety,
    pub facts: Facts,
    pub diagnoses: Vec<Diagnosis>,
}

/// Баланс разнообразия. Обе величины — «за окно», чтобы их можно было сравнивать.
pub fn variety(facts: &Facts) -> Variety {
    let demand = facts.ready as i64
        + facts.stranded as i64
        + facts.unverifiable as i64
        + facts.mismatched as i64
        + facts.algedonic as i64
        + facts.escalations as i64;
    let capacity = facts.running as i64 + facts.confirmed as i64 + facts.in_flight as i64;
    Variety {
        demand,
        capacity,
        gap: capacity - demand,
    }
}

fn diagnosis(
    code: &str,
    system: System,
    severity: Severity,
    text: String,
    evidence: Vec<String>,
    advice: &str,
) -> Diagnosis {
    Diagnosis {
        code: code.to_string(),
        system,
        severity,
        text,
        evidence,
        advice: advice.to_string(),
    }
}

/// Диагноз по следам контуров. Пустой вектор — все контуры подают признаки жизни.
pub fn diagnose(facts: &Facts) -> Vec<Diagnosis> {
    let mut out: Vec<Diagnosis> = Vec::new();

    // --- входы: прибор обязан сказать, чего он не видел -------------------------
    for input in facts.inputs.iter().filter(|state| !state.loaded) {
        out.push(diagnosis(
            "INPUT_MISSING",
            System::Whole,
            Severity::Degraded,
            format!("вход «{}» не прочитан: {}", input.name, input.note),
            vec![format!("источник: {}", input.source)],
            "без этого входа контур(ы) не наблюдаемы: подключи источник или скажи, что его нет",
        ));
    }

    // --- S1: операции ----------------------------------------------------------
    if facts.running == 0 && facts.in_flight == 0 {
        out.push(diagnosis(
            "S1_QUIET",
            System::S1,
            Severity::Degraded,
            "в операциях нет работы: ни одной задачи в исполнении и ни одной взятой под аренду".to_string(),
            vec![
                format!("running = {}", facts.running),
                format!("in_flight = {}", facts.in_flight),
            ],
            "либо всё правда сделано, либо некому работать; различи это до того, как начнёшь планировать",
        ));
    }

    // --- S2: согласование ------------------------------------------------------
    if facts.stranded > 0 {
        out.push(diagnosis(
            "S2_OPEN",
            System::S2,
            Severity::Critical,
            format!(
                "согласование разомкнуто: {} задач(и) в ready без воркера дольше {} мин",
                facts.stranded,
                STRANDED_AFTER / 60
            ),
            vec![
                format!("stranded = {}", facts.stranded),
                format!("ready = {}", facts.ready),
            ],
            "это не «нет работы», это работа без исполнителя: проверь assignee и живой ли пул воркеров",
        ));
    }

    // --- S3: управление --------------------------------------------------------
    if facts.ready > 0 && facts.running == 0 {
        out.push(diagnosis(
            "S3_OPEN",
            System::S3,
            Severity::Critical,
            "управление не раздаёт работу: очередь не пуста, но никто не исполняет".to_string(),
            vec![
                format!("ready = {}", facts.ready),
                format!("running = {}", facts.running),
            ],
            "смотри диспетчер: без него ready-очередь растёт, и это выглядит как прогресс",
        ));
    }
    if facts.crashed_runs > 0 {
        out.push(diagnosis(
            "S3_CRASH",
            System::S3,
            Severity::Degraded,
            format!("исполнители падают: {} последовательных падений", facts.crashed_runs),
            vec![format!("crashed_runs = {}", facts.crashed_runs)],
            "падение — это сигнал о среде (диск, память, доступы), а не о дисциплине; сначала среда",
        ));
    }

    // --- S3*: аудит доказательств ---------------------------------------------
    if facts.closed == 0 {
        out.push(diagnosis(
            "S3STAR_QUIET",
            System::S3Star,
            Severity::Degraded,
            "аудит молчит: за окно нет ни одного перепроверенного закрытия".to_string(),
            vec!["closed = 0".to_string()],
            "без независимой перепроверки «сделано» снова становится заявлением; включи team audit",
        ));
    } else if facts.mismatched > 0 {
        out.push(diagnosis(
            "S3STAR_MISMATCH",
            System::S3Star,
            Severity::Critical,
            format!(
                "закрытия не подтверждаются: {} из {} доказательств не воспроизводятся",
                facts.mismatched, facts.closed
            ),
            vec![
                format!("closed = {}", facts.closed),
                format!("mismatched = {}", facts.mismatched),
                format!("unverifiable = {}", facts.unverifiable),
            ],
            "POSIWID: система делает то, что показывает аудит, а не то, что заявлено; верни работу в ready",
        ));
    }

    if facts.unverifiable > 0 {
        out.push(diagnosis(
            "S3STAR_BLIND",
            System::S3Star,
            Severity::Degraded,
            format!(
                "аудит слеп: {} доказательств(а) проверить не удалось",
                facts.unverifiable
            ),
            vec![format!("unverifiable = {}", facts.unverifiable)],
            "доказательство без способа проверки — это заявление; нужен артефакт, который можно перепроверить",
        ));
    }

    // --- S4: среда -------------------------------------------------------------
    if facts.environmental == 0 {
        out.push(diagnosis(
            "S4_QUIET",
            System::S4,
            Severity::Note,
            "разведка среды молчит: за окно ни одного внешнего изменения".to_string(),
            vec!["environmental = 0".to_string()],
            "либо снаружи правда тихо, либо сканер не работает; второе опаснее",
        ));
    }

    // --- S5: политика ----------------------------------------------------------
    if facts.algedonic_limit > 0 && facts.algedonic >= facts.algedonic_limit {
        out.push(diagnosis(
            "S5_ALGEDONIC_SATURATED",
            System::S5,
            Severity::Degraded,
            format!(
                "аварийный канал на пределе: {} сигналов при лимите {}",
                facts.algedonic, facts.algedonic_limit
            ),
            vec![format!("algedonic = {}/{}", facts.algedonic, facts.algedonic_limit)],
            "по Биру алгедоника — про аварию, а не про состояние: если горит постоянно, чини в обычном контуре",
        ));
    }
    if facts.escalations > 0 {
        out.push(diagnosis(
            "S5_WAITING",
            System::S5,
            Severity::Note,
            format!("владельцу ждут ответа {} вопроса(ов)", facts.escalations),
            vec![format!("escalations = {}", facts.escalations)],
            "это единственный канал, где решает человек; от него зависит не меньше, чем от воркеров",
        ));
    }

    // --- разнообразие ----------------------------------------------------------
    let balance = variety(facts);
    if balance.gap < 0 {
        out.push(diagnosis(
            "VARIETY_SHORT",
            System::Whole,
            Severity::Degraded,
            format!(
                "разнообразие не уравновешено: требует внимания {}, команда переваривает {}",
                balance.demand, balance.capacity
            ),
            vec![format!(
                "demand = {} (ready + stranded + unverifiable + mismatched + algedonic + escalations)",
                balance.demand
            ),
            format!(
                "capacity = {} (running + confirmed + in_flight)",
                balance.capacity
            )],
            "по Эшби: либо ослабь входящее разнообразие (перестань брать новое), либо усиль регулятор (больше или лучше воркеров)",
        ));
    }

    out
}

pub fn report(facts: Facts, taken_at: u64, window: u64) -> Report {
    let diagnoses = diagnose(&facts);
    let verdict = diagnoses
        .iter()
        .map(|item| item.severity)
        .max()
        .unwrap_or(Severity::Ok);
    let balance = variety(&facts);
    Report {
        taken_at,
        window,
        verdict,
        variety: balance,
        facts,
        diagnoses,
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Facts {
        Facts {
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
        }
    }

    fn codes(facts: &Facts) -> Vec<String> {
        diagnose(facts).into_iter().map(|item| item.code).collect()
    }

    #[test]
    fn healthy_team_produces_no_diagnoses() {
        assert!(codes(&healthy()).is_empty(), "{:?}", codes(&healthy()));
        assert_eq!(report(healthy(), 0, 3600).verdict, Severity::Ok);
    }

    #[test]
    fn quiet_operations_is_a_diagnosis_not_a_zero() {
        let mut facts = healthy();
        facts.running = 0;
        facts.in_flight = 0;
        assert!(codes(&facts).contains(&"S1_QUIET".to_string()));
    }

    #[test]
    fn stranded_cards_break_coordination() {
        let mut facts = healthy();
        facts.stranded = 3;
        facts.ready = 3;
        let found = diagnose(&facts);
        let s2 = found.iter().find(|item| item.code == "S2_OPEN").unwrap();
        assert_eq!(s2.severity, Severity::Critical);
        assert!(s2.text.contains('3'));
    }

    #[test]
    fn ready_without_work_is_control_failure() {
        let mut facts = healthy();
        facts.running = 0;
        facts.in_flight = 1; // S1 жив, но управление не раздаёт
        facts.ready = 5;
        assert!(codes(&facts).contains(&"S3_OPEN".to_string()));
    }

    #[test]
    fn silence_of_audit_is_worse_than_no_audit() {
        let mut facts = healthy();
        facts.closed = 0;
        facts.confirmed = 0;
        assert!(codes(&facts).contains(&"S3STAR_QUIET".to_string()));
    }

    #[test]
    fn mismatch_means_the_team_does_what_it_does_not_what_it_says() {
        let mut facts = healthy();
        facts.closed = 4;
        facts.confirmed = 1;
        facts.mismatched = 3;
        let found = diagnose(&facts);
        let s3 = found
            .iter()
            .find(|item| item.code == "S3STAR_MISMATCH")
            .unwrap();
        assert_eq!(s3.severity, Severity::Critical);
        assert!(s3.advice.contains("POSIWID"));
    }

    #[test]
    fn unverifiable_evidence_is_named_blindness() {
        let mut facts = healthy();
        facts.unverifiable = 3;
        let found = diagnose(&facts);
        let blind = found
            .iter()
            .find(|item| item.code == "S3STAR_BLIND")
            .unwrap();
        assert_eq!(blind.severity, Severity::Degraded);
        assert!(blind.text.contains('3'));
    }

    #[test]
    fn blind_environment_is_a_note_not_a_crisis() {
        let mut facts = healthy();
        facts.environmental = 0;
        let found = diagnose(&facts);
        let s4 = found.iter().find(|item| item.code == "S4_QUIET").unwrap();
        assert_eq!(s4.severity, Severity::Note);
    }

    #[test]
    fn algedonic_at_limit_is_not_an_alarm_any_more() {
        let mut facts = healthy();
        facts.algedonic = 3;
        facts.algedonic_limit = 3;
        assert!(codes(&facts).contains(&"S5_ALGEDONIC_SATURATED".to_string()));
    }

    #[test]
    fn missing_input_is_reported_as_missing() {
        let mut facts = healthy();
        facts.inputs = vec![InputState {
            name: "kanban".into(),
            source: "/tmp/kanban.json".into(),
            loaded: false,
            note: "файла нет".into(),
        }];
        let found = diagnose(&facts);
        assert_eq!(found[0].code, "INPUT_MISSING");
        assert_eq!(found[0].severity, Severity::Degraded);
    }

    #[test]
    fn variety_shortfall_is_named_with_numbers() {
        let mut facts = healthy();
        facts.running = 1;
        facts.confirmed = 0;
        facts.in_flight = 0;
        facts.ready = 8;
        facts.stranded = 4;
        let balance = variety(&facts);
        assert_eq!(balance.demand, 12);
        assert_eq!(balance.capacity, 1);
        assert!(balance.gap < 0);
        let found = diagnose(&facts);
        let short = found
            .iter()
            .find(|item| item.code == "VARIETY_SHORT")
            .unwrap();
        assert!(short.text.contains("12") && short.text.contains('1'));
    }

    #[test]
    fn verdict_is_the_worst_diagnosis() {
        let mut facts = healthy();
        facts.stranded = 1;
        facts.ready = 1;
        let built = report(facts, 0, 3600);
        assert_eq!(built.verdict, Severity::Critical);
        assert_eq!(built.verdict.code(), 2);
    }

    #[test]
    fn severity_order_and_exit_codes_agree() {
        assert!(Severity::Critical > Severity::Degraded);
        assert!(Severity::Degraded > Severity::Note);
        assert_eq!(Severity::Ok.code(), 0);
        assert_eq!(Severity::Note.code(), 0);
        assert_eq!(Severity::Degraded.code(), 1);
        assert_eq!(Severity::Critical.code(), 2);
    }
}
