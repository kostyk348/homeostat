//! Живая панель: одна HTML-страница, которая пересобирает диагноз на каждый запрос.
//!
//! У канбана есть доска; здесь доска — это **состояние контуров**. Страница ничего не хранит
//! и не меняет: она показывает то, что прибор прочитал в этот момент. Если вход не снят,
//! это видно на панели (красная строка «НЕТ»), а не превращается в ноль.

use crate::{Report, Severity, System};

fn color(severity: Severity) -> &'static str {
    match severity {
        Severity::Ok => "#39d353",
        Severity::Note => "#d3b439",
        Severity::Degraded => "#e8843c",
        Severity::Critical => "#e5484d",
    }
}

fn word(severity: Severity) -> &'static str {
    match severity {
        Severity::Ok => "живо",
        Severity::Note => "замечание",
        Severity::Degraded => "деградация",
        Severity::Critical => "критично",
    }
}

fn worst_for(report: &Report, system: System) -> Severity {
    report
        .diagnoses
        .iter()
        .filter(|item| item.system == system)
        .map(|item| item.severity)
        .max()
        .unwrap_or(Severity::Ok)
}

pub fn render(report: &Report, not_connected: &[String]) -> String {
    let gap_text = if report.variety.gap < 0 {
        format!("не хватает {}", -report.variety.gap)
    } else {
        format!("запас {}", report.variety.gap)
    };
    let total = (report.variety.demand + report.variety.capacity).max(1);
    let demand_pct = (report.variety.demand.max(0) * 100 / total).min(100);
    let verdict_color = color(report.verdict);

    let mut contours = String::new();
    for system in [
        System::S1,
        System::S2,
        System::S3,
        System::S3Star,
        System::S4,
        System::S5,
        System::Whole,
    ] {
        let severity = worst_for(report, system);
        contours.push_str(&format!(
            r#"<div class="cell" style="border-color:{color}"><div class="sys">{sys}</div><div class="state" style="color:{color}">{word}</div></div>"#,
            color = color(severity),
            sys = system.as_str(),
            word = word(severity)
        ));
    }

    let mut diagnoses = String::new();
    if report.diagnoses.is_empty() {
        diagnoses
            .push_str(r#"<div class="card ok">Контуры подают признаки жизни: жалоб нет.</div>"#);
    }
    for item in &report.diagnoses {
        let mut evidence = String::new();
        for line in &item.evidence {
            evidence.push_str(&format!("<li>{}</li>", escape(line)));
        }
        diagnoses.push_str(&format!(
            r#"<div class="card" style="border-left-color:{color}">
  <div class="row"><span class="tag" style="color:{color}">{system} · {severity}</span><span class="code">{code}</span></div>
  <div class="text">{text}</div>
  <ul class="ev">{evidence}</ul>
  <div class="advice">→ {advice}</div>
</div>"#,
            color = color(item.severity),
            system = item.system.as_str(),
            severity = item.severity.as_str(),
            code = escape(&item.code),
            text = escape(&item.text),
            evidence = evidence,
            advice = escape(&item.advice)
        ));
    }

    let mut inputs = String::new();
    for state in &report.facts.inputs {
        let (mark, tone) = if state.loaded {
            ("ok", "#39d353")
        } else {
            ("НЕТ", "#e5484d")
        };
        inputs.push_str(&format!(
            r#"<tr><td class="name">{name}</td><td style="color:{tone}">{mark}</td><td class="src">{src}</td><td class="note">{note}</td></tr>"#,
            name = escape(&state.name),
            tone = tone,
            mark = mark,
            src = escape(&state.source),
            note = escape(&state.note)
        ));
    }
    for name in not_connected {
        inputs.push_str(&format!(
            r#"<tr><td class="name">{name}</td><td style="color:#8b949e">—</td><td class="src"></td><td class="note">не подключён</td></tr>"#,
            name = escape(name)
        ));
    }

    format!(
        r#"<!doctype html>
<html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="15">
<title>homeostat — живучесть команды</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{ margin:0; padding:24px; background:#0d1117; color:#e6edf3;
         font:14px/1.45 ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif; }}
  h1 {{ font-size:17px; margin:0 0 2px; letter-spacing:.2px; }}
  .sub {{ color:#8b949e; font-size:12px; margin-bottom:18px; }}
  .verdict {{ display:inline-block; padding:3px 10px; border-radius:999px; font-weight:700;
              font-size:12px; color:#0d1117; background:{verdict_color}; vertical-align:middle; margin-left:8px; }}
  .panel {{ background:#161b22; border:1px solid #30363d; border-radius:10px; padding:14px 16px; margin-bottom:14px; }}
  .panel h2 {{ font-size:12px; text-transform:uppercase; letter-spacing:.08em; color:#8b949e; margin:0 0 10px; font-weight:600; }}
  .strip {{ display:grid; grid-template-columns:repeat(7,1fr); gap:8px; }}
  .cell {{ border:1px solid #30363d; border-left-width:4px; border-radius:8px; padding:8px 10px; background:#0d1117; }}
  .sys {{ font-weight:700; font-size:13px; }}
  .state {{ font-size:11px; margin-top:2px; }}
  .bar {{ height:10px; border-radius:999px; background:#21262d; overflow:hidden; margin:8px 0 6px; }}
  .bar span {{ display:block; height:100%; background:linear-gradient(90deg,#e8843c,#e5484d); }}
  .nums {{ display:flex; gap:18px; font-variant-numeric:tabular-nums; }}
  .nums b {{ font-size:18px; }}
  .card {{ background:#0d1117; border:1px solid #30363d; border-left:4px solid #30363d;
           border-radius:8px; padding:10px 12px; margin-bottom:10px; }}
  .card.ok {{ border-left-color:#39d353; color:#9be9a8; }}
  .row {{ display:flex; justify-content:space-between; gap:10px; }}
  .tag {{ font-size:11px; font-weight:700; text-transform:uppercase; letter-spacing:.06em; }}
  .code {{ font:11px ui-monospace,SFMono-Regular,Menlo,monospace; color:#8b949e; }}
  .text {{ margin:6px 0 4px; }}
  .ev {{ margin:2px 0 6px; padding-left:16px; color:#8b949e; }}
  .ev li {{ font:11.5px ui-monospace,SFMono-Regular,Menlo,monospace; }}
  .advice {{ color:#a5d6ff; font-size:12.5px; }}
  table {{ width:100%; border-collapse:collapse; font-size:12px; }}
  th {{ text-align:left; color:#8b949e; font-weight:600; padding:4px 8px; border-bottom:1px solid #30363d; }}
  td {{ padding:4px 8px; border-bottom:1px solid #21262d; vertical-align:top; }}
  td.name {{ font-weight:600; white-space:nowrap; }}
  td.src, td.note {{ color:#8b949e; font-family:ui-monospace,SFMono-Regular,Menlo,monospace; font-size:11px; }}
  .foot {{ color:#6e7681; font-size:11px; margin-top:6px; }}
</style></head><body>
  <h1>homeostat <span class="verdict">{verdict_word}</span></h1>
  <div class="sub">окно {window}с · снято {taken} (unix) · обновление каждые 15с · код выхода {exit_code}</div>

  <div class="panel">
    <h2>контуры (VSM Бира)</h2>
    <div class="strip">{contours}</div>
  </div>

  <div class="panel">
    <h2>разнообразие (закон Эшби): {gap_text}</h2>
    <div class="bar"><span style="width:{demand_pct}%"></span></div>
    <div class="nums">
      <div>требует внимания <b>{demand}</b></div>
      <div>переваривает <b>{capacity}</b></div>
      <div>работа: в пространстве <b>{in_space}</b> · в работе <b>{in_flight}</b></div>
    </div>
    <div class="foot">demand = ready + stranded + unverifiable + mismatched + algedonic + escalations;
      capacity = running + confirmed + in_flight</div>
  </div>

  <div class="panel">
    <h2>диагноз</h2>
    {diagnoses}
  </div>

  <div class="panel">
    <h2>входы</h2>
    <table><tr><th>вход</th><th>состояние</th><th>источник</th><th>что прочитано</th></tr>{inputs}</table>
  </div>

  <div class="foot">homeostat ничего не запускает и не меняет: только читает.
    Прибор — <a href="https://github.com/kostyk348/homeostat" style="color:#58a6ff">homeostat</a>,
    организация работы — <a href="https://github.com/kostyk348/tupleboard" style="color:#58a6ff">tupleboard</a>.</div>
</body></html>"#,
        verdict_color = verdict_color,
        verdict_word = word(report.verdict),
        window = report.window,
        taken = report.taken_at,
        exit_code = report.verdict.code(),
        contours = contours,
        gap_text = gap_text,
        demand_pct = demand_pct,
        demand = report.variety.demand,
        capacity = report.variety.capacity,
        in_space = report.facts.in_space,
        in_flight = report.facts.in_flight,
        diagnoses = diagnoses,
        inputs = inputs
    )
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{report, Facts};

    #[test]
    fn page_contains_contours_and_numbers() {
        let mut facts = Facts {
            running: 1,
            ready: 4,
            stranded: 2,
            closed: 2,
            confirmed: 1,
            mismatched: 1,
            environmental: 0,
            algedonic: 3,
            algedonic_limit: 3,
            ..Facts::default()
        };
        facts.inputs = vec![crate::InputState {
            name: "kanban".into(),
            source: "/tmp/k.json".into(),
            loaded: true,
            note: "36 карточек".into(),
        }];
        let html = render(&report(facts, 1, 3600), &["tupleboard".to_string()]);
        assert!(html.contains("S3*"), "контуры подписаны по VSM");
        assert!(html.contains("S2_OPEN"), "коды диагнозов видны");
        assert!(html.contains("уже 15с") || html.contains("каждые 15с"));
        assert!(
            html.contains("не подключён"),
            "неподключённый вход честно назван"
        );
        assert!(html.contains("критично"));
    }

    #[test]
    fn html_escapes_diagnosis_text() {
        let mut facts = Facts::default();
        facts.inputs = vec![crate::InputState {
            name: "evil".into(),
            source: "<script>alert(1)</script>".into(),
            loaded: false,
            note: "x".into(),
        }];
        let html = render(&report(facts, 0, 3600), &[]);
        assert!(
            !html.contains("<script>alert(1)</script>"),
            "данные не становятся разметкой"
        );
        assert!(html.contains("&lt;script&gt;"));
    }
}
