//! `cargo xtask report`: the gate's performance budgets.
//!
//! - Runtime overhead per step on a 200-event session: under 1 ms. Measured
//!   for the loop alone and for the whole host (MemoryHost: store, publish,
//!   dispatch), with an instant scripted model, so the time is the runtime's.
//! - Replaying 10,000 events (long-thread: decode the JSONL generation and
//!   rebuild its last request): under 100 ms.
//!
//! Each figure is the median of five runs.

use harness_core::*;
use harness_host::{AppKind, CreateOptions, Input as HostInput, MemoryHost};
use harness_log::history::request_for;
use harness_log::{decode, ModelEvent, ModelStop};
use harness_loop::{Config, Input, Output, Session};
use harness_tools::{ToolDone, ToolOutput};
use std::time::{Duration, Instant};

const TURNS: usize = 17;

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn usage() -> Usage {
    Usage {
        input_tokens: 100,
        output_tokens: 10,
        ..Usage::default()
    }
}

/// The loop alone: (ms per step, events, steps).
fn loop_run() -> (f64, usize, usize) {
    let preset = harness_preset::builtin("chat")
        .and_then(|(_, p)| p.preset)
        .expect("chat preset");
    let now_tool = ToolSpec {
        name: "now".into(),
        description: "The time.".into(),
        input_schema: Value::object([("type", Value::from("object"))]),
        kind: ToolKind::Engine,
        effects: vec![],
        concurrency: Concurrency::Parallel,
    };
    let cfg = Config::new(preset.clone(), Some(Profile::Standard), vec![now_tool]);
    let start = SessionStart {
        class: SessionClass::Interactive,
        app: AppId::from("chat"),
        principal: Principal {
            kind: PrincipalKind::User,
            id: None,
        },
        preset: preset.reference.clone(),
        parent: None,
        profile: Some(Profile::Standard),
    };
    let mut t: Millis = 1_794_646_800_000;
    let (mut s, _) = Session::create(cfg, SessionId::from("ses_report"), start, t);
    let mut spent = Duration::ZERO;
    let mut steps = 0;
    let mut feed = |s: &mut Session, input: Input| -> Vec<Output> {
        t += 50;
        let t0 = Instant::now();
        let out = s.handle(t, input);
        spent += t0.elapsed();
        out
    };
    for i in 0..TURNS {
        let ctx = RequestContext {
            date: "2026-11-15".into(),
            time_zone: "America/New_York".into(),
            language: "en".into(),
            location: None,
        };
        feed(
            &mut s,
            Input::User {
                content: vec![Block::text(format!("Question {i}: what time is it?"))],
                context: Some(ctx),
            },
        );
        feed(
            &mut s,
            Input::Model(ModelEvent::ToolCall {
                tool: "now".into(),
                input: "{}".into(),
            }),
        );
        let outs = feed(
            &mut s,
            Input::Model(ModelEvent::Done {
                stop: ModelStop::ToolUse,
                usage: usage(),
            }),
        );
        let call = outs.iter().find_map(|o| match o {
            Output::RunTool { call, .. } => Some(call.call.clone()),
            _ => None,
        });
        if let Some(call) = call {
            feed(
                &mut s,
                Input::Tool {
                    call,
                    output: ToolOutput::Done(ToolDone::ok("10:00")),
                },
            );
        }
        feed(
            &mut s,
            Input::Model(ModelEvent::Text(format!("It is 10:00 (answer {i})."))),
        );
        feed(
            &mut s,
            Input::Model(ModelEvent::Done {
                stop: ModelStop::End,
                usage: usage(),
            }),
        );
        if i == 0 {
            // The title step.
            feed(
                &mut s,
                Input::Model(ModelEvent::Text("Time questions".into())),
            );
            feed(
                &mut s,
                Input::Model(ModelEvent::Done {
                    stop: ModelStop::End,
                    usage: usage(),
                }),
            );
        }
    }
    for e in s.events() {
        if matches!(e.event, Event::StepStart(_)) {
            steps += 1;
        }
    }
    (
        spent.as_secs_f64() * 1000.0 / steps as f64,
        s.events().len(),
        steps,
    )
}

/// The whole host over MemoryHost: (ms per step, events, steps).
fn host_run() -> (f64, usize, usize) {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = m
        .block_on(
            m.host
                .create_session(&a, &PresetId::from("chat"), CreateOptions::default()),
        )
        .expect("session");
    for i in 0..TURNS {
        m.model.calls(&[("now", "{}")]);
        m.model.text(&format!("It is 10:00 (answer {i})."));
        if i == 0 {
            m.model.text("Time questions");
        }
    }
    let t0 = Instant::now();
    for i in 0..TURNS {
        let input = HostInput {
            content: vec![Block::text(format!("Question {i}: what time is it?"))],
            client_ref: None,
        };
        m.block_on(m.host.send(&a, &s, input)).expect("send");
    }
    let spent = t0.elapsed();
    let log = m.log(&s);
    let steps = log
        .iter()
        .filter(|e| matches!(e.event, Event::StepStart(_)))
        .count();
    (
        spent.as_secs_f64() * 1000.0 / steps as f64,
        log.len(),
        steps,
    )
}

/// Decode long-thread and rebuild its last request: (ms, events).
fn replay_run(jsonl: &str) -> (f64, usize) {
    let t0 = Instant::now();
    let g = decode(jsonl).expect("long-thread decodes");
    let last = g
        .events
        .iter()
        .rev()
        .find(|e| matches!(e.event, Event::StepStart(_)))
        .and_then(|e| e.step.clone())
        .expect("a step");
    let req = request_for(&g.events, &last).expect("rebuilds");
    std::hint::black_box(req.bytes());
    (t0.elapsed().as_secs_f64() * 1000.0, g.events.len())
}

pub fn run() -> Result<(), String> {
    // The budgets are for optimized builds: a debug xtask re-runs itself in
    // release (set ISONOMIC_REPORT_DEBUG=1 to measure the debug build).
    if cfg!(debug_assertions) && std::env::var_os("ISONOMIC_REPORT_DEBUG").is_none() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = std::process::Command::new(cargo)
            .args([
                "run",
                "--release",
                "--locked",
                "--quiet",
                "--package",
                "xtask",
                "--",
                "report",
            ])
            .current_dir(crate::workspace_root())
            .status()
            .map_err(|e| e.to_string())?;
        return if status.success() {
            Ok(())
        } else {
            Err("a budget was exceeded".into())
        };
    }
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("cargo xtask report ({profile} build, median of 5 runs)\n");
    let mut lp = Vec::new();
    let mut hp = Vec::new();
    let mut rp = Vec::new();
    let (mut le, mut ls, mut he, mut hs, mut re) = (0, 0, 0, 0, 0);
    let long = harness_testkit::fixtures::by_name("long-thread")
        .ok_or("no long-thread")?
        .jsonl();
    for _ in 0..5 {
        let (ms, e, s) = loop_run();
        lp.push(ms);
        (le, ls) = (e, s);
        let (ms, e, s) = host_run();
        hp.push(ms);
        (he, hs) = (e, s);
        let (ms, e) = replay_run(&long);
        rp.push(ms);
        re = e;
    }
    let rows = [
        (
            format!("loop overhead per step ({le} events, {ls} steps)"),
            median(lp),
            1.0,
        ),
        (
            format!("host overhead per step ({he} events, {hs} steps)"),
            median(hp),
            1.0,
        ),
        (
            format!("replay {re} events (decode + rebuild)"),
            median(rp),
            100.0,
        ),
    ];
    let mut ok = true;
    println!("| Measure | Result | Budget | |\n| --- | --- | --- | --- |");
    for (name, value, budget) in rows {
        let pass = value < budget;
        ok &= pass;
        println!(
            "| {name} | {value:.3} ms | < {budget} ms | {} |",
            if pass { "pass" } else { "FAIL" }
        );
    }
    if ok {
        Ok(())
    } else {
        Err("a budget was exceeded".into())
    }
}
