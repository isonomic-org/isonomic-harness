//! `cargo xtask smoke`: one two-step turn with a tool call, through the
//! loop, against a local model server: the llama.cpp server that
//! isonomic-inference's local provider launches (OpenAI-style chat
//! completions on loopback, with a bearer secret).
//!
//! Environment:
//! - `ISONOMIC_SMOKE_URL`: the server, default `http://127.0.0.1:8080`.
//! - `ISONOMIC_SMOKE_KEY`: the bearer secret, if the server wants one.
//! - `ISONOMIC_SMOKE_MODEL`: the model name to report, default `local`.
//!
//! Prints a block to paste back; exits non-zero unless every check passes.

use harness_core::*;
use harness_log::history::request_for;
use harness_log::{ModelEvent, ModelRequest, ModelStop, Part, Role};
use harness_loop::{Config, Input, Output, Session};
use harness_tools::{Tool, ToolCtx};
use serde_json::{json, Value as Json};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Instant;

fn text_of(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::Text { text } | Part::Context { text } | Part::Summary { text } => {
                Some(text.clone())
            }
            Part::Attachment { name, text, .. } => Some(format!("[{name}]\n{text}")),
            Part::Mention { label, .. } => Some(label.clone()),
            Part::Json { value } => serde_json::to_string(value).ok(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The request in chat-completions form.
fn body(req: &ModelRequest) -> Json {
    let mut messages = vec![json!({"role": "system", "content": req.system})];
    for m in &req.messages {
        match m.role {
            Role::User => messages.push(json!({"role": "user", "content": text_of(&m.content)})),
            Role::Assistant => {
                let calls: Vec<Json> = m
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        Part::ToolCall { call, tool, input } => Some(json!({
                            "id": call.as_str(), "type": "function",
                            "function": {"name": tool, "arguments": serde_json::to_string(input).unwrap_or_default()}
                        })),
                        _ => None,
                    })
                    .collect();
                let mut msg = json!({"role": "assistant", "content": text_of(&m.content)});
                if !calls.is_empty() {
                    msg["tool_calls"] = Json::Array(calls);
                }
                messages.push(msg);
            }
            Role::Tool => {
                for p in &m.content {
                    if let Part::ToolResult { call, content, .. } = p {
                        messages.push(json!({"role": "tool", "tool_call_id": call.as_str(), "content": text_of(content)}));
                    }
                }
            }
        }
    }
    let tools: Vec<Json> = req
        .tools
        .iter()
        .map(|t| {
            json!({"type": "function", "function": {
                "name": t.name, "description": t.description,
                "parameters": serde_json::to_value(&t.input_schema).unwrap_or(Json::Null)
            }})
        })
        .collect();
    let mut b = json!({"model": req.model, "messages": messages, "stream": false,
        "max_tokens": req.params.max_output_tokens});
    if !tools.is_empty() {
        b["tools"] = Json::Array(tools);
    }
    b
}

fn post(url: &str, key: Option<&str>, payload: &Json) -> Result<Json, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("ISONOMIC_SMOKE_URL must be http://host:port")?;
    let host = rest.trim_end_matches('/');
    let mut stream = TcpStream::connect(host).map_err(|e| format!("connect {host}: {e}"))?;
    let data = payload.to_string();
    let auth = key
        .map(|k| format!("Authorization: Bearer {k}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|_| stream.write_all(data.as_bytes()))
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw);
    let (headers, mut payload) = text.split_once("\r\n\r\n").ok_or("no HTTP body")?;
    let status = headers.lines().next().unwrap_or("");
    if !status.contains(" 200 ") {
        return Err(format!(
            "server answered {status}: {}",
            payload.chars().take(300).collect::<String>()
        ));
    }
    let dechunked;
    if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut out = String::new();
        let mut rest = payload;
        while let Some((size, tail)) = rest.split_once("\r\n") {
            let n = usize::from_str_radix(size.trim(), 16).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            out.push_str(tail.get(..n).ok_or("short chunk")?);
            rest = tail.get(n + 2..).unwrap_or("");
        }
        dechunked = out;
        payload = &dechunked;
    }
    serde_json::from_str(payload).map_err(|e| format!("bad JSON from the server: {e}"))
}

/// The server's answer as model events.
fn events(answer: &Json) -> Vec<ModelEvent> {
    let choice = &answer["choices"][0];
    let msg = &choice["message"];
    let mut out = Vec::new();
    if let Some(r) = msg["reasoning_content"].as_str().filter(|s| !s.is_empty()) {
        out.push(ModelEvent::Reasoning(r.into()));
    }
    if let Some(t) = msg["content"].as_str().filter(|s| !s.is_empty()) {
        out.push(ModelEvent::Text(t.into()));
    }
    for c in msg["tool_calls"].as_array().into_iter().flatten() {
        out.push(ModelEvent::ToolCall {
            tool: c["function"]["name"].as_str().unwrap_or("").into(),
            input: c["function"]["arguments"].as_str().unwrap_or("{}").into(),
        });
    }
    let stop = match choice["finish_reason"].as_str() {
        Some("tool_calls") => ModelStop::ToolUse,
        Some("length") => ModelStop::MaxTokens,
        _ if out.iter().any(|e| matches!(e, ModelEvent::ToolCall { .. })) => ModelStop::ToolUse,
        _ => ModelStop::End,
    };
    let u = &answer["usage"];
    out.push(ModelEvent::Done {
        stop,
        usage: Usage {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
            ..Usage::default()
        },
    });
    out
}

fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run() -> Result<(), String> {
    let url =
        std::env::var("ISONOMIC_SMOKE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let key = std::env::var("ISONOMIC_SMOKE_KEY").ok();
    let model = std::env::var("ISONOMIC_SMOKE_MODEL").unwrap_or_else(|_| "local".into());
    let now_tool = harness_toolkit::Now::new(std::sync::Arc::new(harness_toolkit::FixedZone {
        name: "UTC".into(),
        offset_minutes: 0,
    }));
    let mut preset = harness_preset::builtin("chat")
        .and_then(|(_, p)| p.preset)
        .ok_or("chat preset")?;
    preset.model.id = model.clone();
    preset.model.max_output_tokens = 256;
    preset.tools.allow = vec!["now".into()];
    let cfg = Config::new(preset.clone(), Some(Profile::Intel), vec![now_tool.spec()]);
    let start = SessionStart {
        class: SessionClass::Interactive,
        app: AppId::from("smoke"),
        principal: Principal {
            kind: PrincipalKind::User,
            id: None,
        },
        preset: preset.reference.clone(),
        parent: None,
        profile: Some(Profile::Intel),
    };
    let id = SessionId::from("ses_smoke");
    let (mut s, _) = Session::create(cfg, id.clone(), start, now_ms());
    let ask = "What time is it in UTC? Call the now tool first, then answer in one short sentence.";
    let mut pending = vec![Input::User {
        content: vec![Block::text(ask)],
        context: None,
    }];
    let mut sent: Vec<(StepId, ModelRequest, f64)> = Vec::new();
    let mut calls = Vec::new();
    let mut guard = 0;
    while let Some(input) = pending.pop() {
        for o in s.handle(now_ms(), input) {
            match o {
                Output::Request { step, request } => {
                    guard += 1;
                    if guard > 6 {
                        return Err("more than six requests; stopping".into());
                    }
                    let t0 = Instant::now();
                    let answer = post(&url, key.as_deref(), &body(&request))?;
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    sent.push((step, request, ms));
                    pending.extend(events(&answer).into_iter().rev().map(Input::Model));
                }
                Output::RunTool { call, .. } => {
                    calls.push(call.tool.clone());
                    let cx = ToolCtx {
                        session: id.clone(),
                        app: AppId::from("smoke"),
                        call: call.call.clone(),
                        now: now_ms(),
                        child: None,
                    };
                    let mut fut = now_tool.call(&call.input, &cx);
                    let waker = std::task::Waker::noop();
                    let output = match fut
                        .as_mut()
                        .poll(&mut std::task::Context::from_waker(waker))
                    {
                        std::task::Poll::Ready(o) => o,
                        std::task::Poll::Pending => {
                            return Err("the now tool did not finish".into())
                        }
                    };
                    pending.insert(
                        0,
                        Input::Tool {
                            call: call.call,
                            output,
                        },
                    );
                }
                _ => {}
            }
        }
    }
    let events = s.events();
    let end = events.iter().rev().find_map(|e| match &e.event {
        Event::TurnEnd(t) => Some(t.stop.clone()),
        _ => None,
    });
    let answer = events.iter().rev().find_map(|e| match &e.event {
        Event::Assistant(a) => Some(
            a.content
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<String>(),
        ),
        _ => None,
    });
    let replay_ok = sent.iter().all(|(step, req, _)| {
        request_for(events, step)
            .map(|r| r.bytes() == req.bytes())
            .unwrap_or(false)
    });
    let checks = [
        ("turn ended done", end == Some(TurnStop::Done)),
        ("two or more steps", sent.len() >= 2),
        ("the now tool was called", calls.iter().any(|c| c == "now")),
        ("replay rebuilt every request", replay_ok),
    ];
    println!("---- isonomic-harness smoke ----");
    println!("server: {url}\nmodel: {model}");
    for (i, (_, req, ms)) in sent.iter().enumerate() {
        println!(
            "request {}: {} messages, {:.0} ms",
            i + 1,
            req.messages.len(),
            ms
        );
    }
    println!("tool calls: {calls:?}");
    println!("answer: {}", answer.unwrap_or_default().trim());
    let mut ok = true;
    for (name, pass) in checks {
        ok &= pass;
        println!("{} {name}", if pass { "pass" } else { "FAIL" });
    }
    println!("{}", if ok { "SMOKE PASS" } else { "SMOKE FAIL" });
    println!("--------------------------------");
    if ok {
        Ok(())
    } else {
        Err("smoke failed".into())
    }
}
