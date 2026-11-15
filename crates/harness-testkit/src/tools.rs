//! The tools the fixtures offer the model: the Milestone 0 toolkit tools and
//! fake tools with fixed outputs.

use harness_core::*;

fn spec(
    name: &str,
    description: &str,
    props: Vec<(&str, &str)>,
    kind: ToolKind,
    effects: Vec<Effect>,
    concurrency: Concurrency,
) -> ToolSpec {
    let properties = Value::object(
        props
            .iter()
            .map(|(k, t)| (*k, Value::object([("type", Value::from(*t))]))),
    );
    let required = Value::from(props.iter().map(|(k, _)| *k).collect::<Vec<_>>());
    ToolSpec {
        name: name.to_owned(),
        description: description.to_owned(),
        input_schema: Value::object([
            ("type", Value::from("object")),
            ("properties", properties),
            ("required", required),
        ]),
        kind,
        effects,
        concurrency,
    }
}

fn engine(name: &str, description: &str, props: Vec<(&str, &str)>) -> ToolSpec {
    spec(
        name,
        description,
        props,
        ToolKind::Engine,
        vec![],
        Concurrency::Parallel,
    )
}

/// `ask_user`: ask the person a question.
pub fn ask_user() -> ToolSpec {
    engine(
        "ask_user",
        "Ask the person a question, with options when there are clear choices.",
        vec![("prompt", "string"), ("options", "array")],
    )
}

/// `todo`: keep a to-do list for the session.
pub fn todo() -> ToolSpec {
    engine(
        "todo",
        "Add, check off or list to-do items.",
        vec![("action", "string"), ("item", "string")],
    )
}

/// `plan`: record the plan for a multi-step task.
pub fn plan() -> ToolSpec {
    engine(
        "plan",
        "Record or update the plan for a multi-step task.",
        vec![("steps", "array")],
    )
}

/// `now`: the current date and time in the person's time zone.
pub fn now() -> ToolSpec {
    engine(
        "now",
        "The current date and time in the person's time zone.",
        vec![],
    )
}

/// The Milestone 0 toolkit.
pub fn toolkit() -> Vec<ToolSpec> {
    vec![ask_user(), todo(), plan(), now()]
}

/// Fake: search the person's notes.
pub fn search_notes() -> ToolSpec {
    let mut t = engine(
        "search_notes",
        "Search the person's notes.",
        vec![("query", "string")],
    );
    t.effects = vec![Effect::ReadsPrivateData];
    t
}

/// Fake: read one note.
pub fn read_note() -> ToolSpec {
    let mut t = engine("read_note", "Read a note by id.", vec![("id", "string")]);
    t.effects = vec![Effect::ReadsPrivateData];
    t
}

/// Fake: fetch a supplier's price.
pub fn fetch_quote() -> ToolSpec {
    let mut t = engine(
        "fetch_quote",
        "Fetch a supplier's current price for an item.",
        vec![("item", "string")],
    );
    t.effects = vec![Effect::IngestsUntrustedContent];
    t
}

/// Fake: create an artifact.
pub fn artifact_create() -> ToolSpec {
    spec(
        "artifact_create",
        "Create a document artifact.",
        vec![("title", "string"), ("body", "string")],
        ToolKind::Engine,
        vec![],
        Concurrency::Serial,
    )
}

/// Fake: update an artifact.
pub fn artifact_update() -> ToolSpec {
    spec(
        "artifact_update",
        "Replace an artifact's body with a new version.",
        vec![("artifact", "string"), ("body", "string")],
        ToolKind::Engine,
        vec![],
        Concurrency::Serial,
    )
}

/// Fake: send an email (acts outward).
pub fn send_email() -> ToolSpec {
    spec(
        "send_email",
        "Send an email from the person's account.",
        vec![("to", "string"), ("subject", "string"), ("body", "string")],
        ToolKind::Engine,
        vec![Effect::ActsOutward],
        Concurrency::Serial,
    )
}

/// Fake: post a message to a channel (acts outward).
pub fn post_message() -> ToolSpec {
    spec(
        "post_message",
        "Post a message to one of the person's channels.",
        vec![("channel", "string"), ("text", "string")],
        ToolKind::Engine,
        vec![Effect::ActsOutward],
        Concurrency::Serial,
    )
}

/// Fake: collect items from a source.
pub fn collect_items() -> ToolSpec {
    let mut t = engine(
        "collect_items",
        "Collect new items from a source since a date.",
        vec![("source", "string"), ("since", "string")],
    );
    t.effects = vec![Effect::ReadsPrivateData, Effect::IngestsUntrustedContent];
    t
}

/// Fake: delegate a task to a subagent.
pub fn delegate() -> ToolSpec {
    spec(
        "delegate",
        "Hand a self-contained task to a subagent and wait for its report.",
        vec![("task", "string")],
        ToolKind::Engine,
        vec![],
        Concurrency::Serial,
    )
}

/// Client tool: insert a block into the open document.
pub fn insert_block() -> ToolSpec {
    spec(
        "insert_block",
        "Insert a block into the open document.",
        vec![("position", "string"), ("block", "object")],
        ToolKind::Client,
        vec![],
        Concurrency::Serial,
    )
}
