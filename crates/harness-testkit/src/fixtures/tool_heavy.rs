// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! tool-heavy: on the Intel profile, a transient model error retried, three
//! parallel calls with one failing, a long result cut by the tool's output
//! limit, an artifact created then updated, and a heuristic title.

use super::build::*;
use crate::recorder::Recorder;
use crate::tools;
use harness_core::*;

const PROMPT: &str =
    "You are a helpful assistant. Be accurate and concise, and use Markdown when it helps.";
const ASK: &str = "Pull together my notes on the garden project and draft a one-page plan.";

const PLAN_V1: &str = "# Garden plan\n\n## Goal\nFour raised cedar beds along the south fence, planted by mid-April.\n\n## Steps\n1. Clear and level the strip (March, one weekend).\n2. Build the beds: 4 × 8 ft, 12 in deep.\n3. Fill with a 60/40 soil and compost mix.\n4. Plant: tomatoes and basil, beans, greens, herbs.\n\n## Open questions\n- Drip line or hand watering?\n- Who waters in July?\n";
const PLAN_V2_EXTRA: &str = "\n## Budget\n| Item | Estimate |\n| --- | --- |\n| Cedar boards | not yet priced |\n| Soil and compost (2 cu yd) | $180 |\n| Drip kit | $65 |\n| Seeds and starts | $70 |\n";

fn long_note() -> (String, Truncation) {
    let mut head = String::from("Garden project, running notes.\n\n");
    for week in 1..=14 {
        head.push_str(&format!(
            "Week {week}: measured the south strip again, checked sun from 9 to 4, noted drainage after rain, and listed what the neighbors grow well.\n"
        ));
    }
    let tail = "Latest: decided on four beds rather than six. Cedar over pine. Ask about the shared hose before buying a drip kit.\n";
    let omitted: u64 = 45_310;
    let marker = format!("\n[… {omitted} bytes omitted …]\n\n");
    let truncation = Truncation {
        original_bytes: head.len() as u64 + omitted + tail.len() as u64,
        head_bytes: head.len() as u64,
        tail_bytes: tail.len() as u64,
    };
    (format!("{head}{marker}{tail}"), truncation)
}

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Intel);
    r.preset_prompt(&mut s, PROMPT);

    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, ASK);
    r.context(&mut s, None);

    // Step 1: a transient error, retried; then three parallel calls.
    let offered = vec![
        tools::search_notes(),
        tools::read_note(),
        tools::fetch_quote(),
        tools::artifact_create(),
        tools::artifact_update(),
    ];
    r.step(&mut s, Purpose::Reply, Some(offered));
    let busy = Attempt {
        outcome: AttemptOutcome::Error,
        error: Some(ErrorInfo::transient(
            "model.busy",
            "The local model was busy.",
        )),
        content: vec![],
        usage: None,
        retry: 0,
        will_retry: true,
    };
    r.attempt(&mut s, busy);
    r.reply(
        &mut s,
        text("I'll search your notes and check a price."),
        StopReason::ToolUse,
        None,
    );
    let search = tools::search_notes();
    let q = |s: &str| Value::object([("query", Value::from(s))]);
    r.call(&mut s, call("cal_0001", &search, q("garden"), Some(0)));
    r.call(&mut s, call("cal_0002", &search, q("raised beds"), Some(0)));
    let item = Value::object([("item", Value::from("cedar boards, 2x10x8"))]);
    r.call(
        &mut s,
        call("cal_0003", &tools::fetch_quote(), item, Some(0)),
    );
    let hits = |ids: &[&str]| {
        let found: Vec<Value> = ids
            .iter()
            .map(|id| Value::object([("id", Value::from(*id)), ("title", Value::from("Garden"))]))
            .collect();
        vec![Block::Json {
            value: Value::object([("results", Value::Array(found))]),
        }]
    };
    r.result(
        &mut s,
        result("cal_0002", ToolOutcome::Ok, hits(&["note_12"])),
        300,
    );
    let mut failed = result(
        "cal_0003",
        ToolOutcome::Error,
        text("fetch_quote failed: the supplier's price list could not be reached. Continue without the price or try again later."),
    );
    failed.error = Some(ErrorInfo::new(
        "tool.unreachable",
        "The supplier's price list could not be reached.",
    ));
    r.result(&mut s, failed, 200);
    r.result(
        &mut s,
        result("cal_0001", ToolOutcome::Ok, hits(&["note_12", "note_31"])),
        150,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);

    // Step 2: a long result, cut by the tool's output limit.
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Reading the main note."),
        StopReason::ToolUse,
        None,
    );
    let id = Value::object([("id", Value::from("note_12"))]);
    r.call(&mut s, call("cal_0004", &tools::read_note(), id, None));
    let (note, truncation) = long_note();
    let mut long = result("cal_0004", ToolOutcome::Ok, text(&note));
    long.truncated = Some(truncation);
    r.result(&mut s, long, 400);
    r.end_step(&mut s, StepOutcome::ToolCalls);

    // Steps 3 and 4: an artifact created, then updated.
    let v2 = format!("{PLAN_V1}{PLAN_V2_EXTRA}");
    let writes = [
        (
            "Drafting the plan.",
            tools::artifact_create(),
            PLAN_V1,
            1,
            ArtifactOp::Create,
        ),
        (
            "Adding a budget section.",
            tools::artifact_update(),
            v2.as_str(),
            2,
            ArtifactOp::Update,
        ),
    ];
    for (n, (say, tool, body, version, op)) in writes.into_iter().enumerate() {
        let call_id = format!("cal_{:04}", 5 + n);
        r.step(&mut s, Purpose::Reply, None);
        r.reply(&mut s, text(say), StopReason::ToolUse, None);
        let input = if version == 1 {
            Value::object([
                ("title", Value::from("Garden plan")),
                ("body", Value::from(body)),
            ])
        } else {
            Value::object([
                ("artifact", Value::from("art_0001")),
                ("body", Value::from(body)),
            ])
        };
        r.call(&mut s, call(&call_id, &tool, input, None));
        r.advance(120);
        let ev = ArtifactEvent {
            artifact: ArtifactId::from("art_0001"),
            version,
            op,
            title: "Garden plan".into(),
            media_type: "text/markdown".into(),
            blob: BlobId::new(format!("blb_{version:04}")),
            bytes: body.len() as u64,
            call: Some(CallId::new(call_id.clone())),
        };
        r.emit(&s, Event::Artifact(ev));
        let done = format!("Saved artifact art_0001, version {version}.");
        r.result(&mut s, result(&call_id, ToolOutcome::Ok, text(&done)), 60);
        r.end_step(&mut s, StepOutcome::ToolCalls);
    }

    // Step 5: the answer.
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Your one-page plan is in **Garden plan** (version 2): four cedar beds on the south fence, built in March and planted by mid-April, with a budget table.\n\nOne gap: I couldn't reach the supplier for the cedar price, so that line is open. Want me to try again later?"),
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);

    // The Intel profile titles by heuristic: the prompt's opening words.
    r.emit_title(
        &mut s,
        "Pull together my notes on the garden project",
        TitleSource::Heuristic,
    );
    r.finish()
}
