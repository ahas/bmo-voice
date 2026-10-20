//! The voice's OpenAPI 3.1 document (`docs/boundary-api.md`, 「The contract」): every route with its parameters,
//! its body, its answer and the codes it can answer, the bodies' schemas written from [`crate::wire`]'s types, the
//! events under `x-events`, the harness plugin under `x-harness` and its tools under `x-tools`, and what the
//! redesign moved out under `x-moved-out`. The repository keeps it as `docs/openapi.json`, which
//! `bmo-voice openapi` prints.

use schemars::JsonSchema;
use schemars::generate::SchemaGenerator;
use serde_json::{Map, Value, json};

use crate::events::KINDS;
use crate::mcp::{ENTRY, MCP, PLUGIN, generator, tools};
use crate::wire::{
  Encoding, Event, EventQuery, Health, NewEncoding, NewScriptReading, NewSynthesis, NewTranscription, Page,
  ScriptReading, Synthesis, Transcription, Voice, VoiceServer, WireError,
};

/// Where the schemas the routes refer to stand in the document.
const SCHEMAS: &str = "/components/schemas";

const DESCRIPTION: &str = "The voice engine: trained voices, speech synthesis on the voice servers of the nodes, \
  transcription, speaker encodings and scripts for the ear. A synthesis is answered at once and the sound follows. \
  Bodies are JSON with camelCase keys, times are integers of epoch milliseconds and spans of milliseconds end in \
  Ms. A list is a page, `after` being the last member's name. Every failure is a non-2xx answer whose body is \
  `code` and `error`. The harness plugin's tools, each a thin call of one route, are stated under `x-tools`. \
  Speaking to listeners on a schedule is a caller's work and calls this engine; the briefings that did so are \
  another service's.";

/// The path's voice.
fn voice_key() -> Value {
  json!({
    "name": "voiceName",
    "in": "path",
    "required": true,
    "description": "The voice's name.",
    "schema": { "type": "string" },
  })
}

/// The path's synthesis.
fn synthesis_key() -> Value {
  json!({
    "name": "synthesisId",
    "in": "path",
    "required": true,
    "description": "The synthesis's id.",
    "schema": { "type": "integer", "format": "int64" },
  })
}

/// The path's transcription.
fn transcription_key() -> Value {
  json!({
    "name": "transcriptionId",
    "in": "path",
    "required": true,
    "description": "The transcription's id.",
    "schema": { "type": "integer", "format": "int64" },
  })
}

/// A list's cursor and size.
fn page_params(cursor: &str) -> Vec<Value> {
  vec![
    json!({
      "name": "after",
      "in": "query",
      "required": false,
      "description": cursor,
      "schema": { "type": "string" },
    }),
    json!({
      "name": "limit",
      "in": "query",
      "required": false,
      "description": "1 to 1,000; 100 where absent.",
      "schema": { "type": "integer" },
    }),
  ]
}

/// The query parameters of a query type.
fn query<Query: JsonSchema>(generator: &mut SchemaGenerator) -> Vec<Value> {
  let schema = Query::json_schema(generator).to_value();
  let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
    return Vec::new();
  };

  properties
    .iter()
    .map(|(name, property)| {
      let mut parameter = json!({ "name": name, "in": "query", "required": false, "schema": property });

      if let Some(description) = property.get("description") {
        parameter["description"] = description.clone();
      }

      parameter
    })
    .collect()
}

fn json_answer<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
  json!({ "application/json": { "schema": generator.subschema_for::<T>() } })
}

/// What a route is asked with: its path and query parameters, and its body where it takes one.
#[derive(Default)]
struct Asked {
  parameters: Vec<Value>,
  body: Option<Value>,
}

/// One operation, its success under `status` with `content`, its failure the document's one error answer.
fn operation(
  id: &str,
  tag: &str,
  summary: &str,
  asked: Asked,
  status: u16,
  content: Value,
  codes: &[&str],
) -> Value {
  let Asked { parameters, body } = asked;
  let mut operation = json!({
    "operationId": id,
    "tags": [tag],
    "summary": summary,
    "responses": {
      status.to_string(): { "description": summary, "content": content },
      "default": { "$ref": "#/components/responses/Error" },
    },
    "x-codes": codes,
  });

  if !parameters.is_empty() {
    operation["parameters"] = parameters.into();
  }

  if let Some(body) = body {
    operation["requestBody"] =
      json!({ "required": true, "content": { "application/json": { "schema": body } } });
  }

  operation
}

/// The OpenAPI document.
pub fn openapi() -> Value {
  let mut generator = generator(SCHEMAS);
  let error = generator.subschema_for::<WireError>();
  let mut paths: Map<String, Value> = Map::new();
  let mut path = |name: &str, method: &str, operation: Value| {
    paths
      .entry(name.to_owned())
      .or_insert_with(|| json!({}))
      .as_object_mut()
      .expect("a path is an object")
      .insert(method.to_owned(), operation);
  };

  path(
    "/health",
    "get",
    operation(
      "readHealth",
      "health",
      "Readiness and version.",
      Asked::default(),
      200,
      json_answer::<Health>(&mut generator),
      &["unauthorized"],
    ),
  );
  path(
    "/events",
    "get",
    operation(
      "listEvents",
      "events",
      "The service's one event stream (SSE), or a page of it with Accept: application/json.",
      Asked {
        parameters: query::<EventQuery>(&mut generator),
        body: None,
      },
      200,
      json!({
        "text/event-stream": { "schema": { "type": "string", "description": "Server-sent events, each an event." } },
        "application/json": { "schema": generator.subschema_for::<Page<Event>>() },
      }),
      &["unauthorized", "bad-request", "gone"],
    ),
  );
  path(
    "/voices",
    "get",
    operation(
      "listVoices",
      "voices",
      "The trained voices, each with its model and emotions.",
      Asked {
        parameters: page_params("The last voice's name; the page starts past it."),
        body: None,
      },
      200,
      json_answer::<Page<Voice>>(&mut generator),
      &["unauthorized", "bad-request"],
    ),
  );
  path(
    "/voices/{voiceName}",
    "get",
    operation(
      "readVoice",
      "voices",
      "One voice.",
      Asked {
        parameters: vec![voice_key()],
        body: None,
      },
      200,
      json_answer::<Voice>(&mut generator),
      &["unauthorized", "not-found"],
    ),
  );
  path(
    "/syntheses",
    "post",
    operation(
      "createSynthesis",
      "syntheses",
      "Speak a text in a voice; the synthesis is answered at once and its sound follows.",
      Asked {
        parameters: Vec::new(),
        body: Some(generator.subschema_for::<NewSynthesis>().to_value()),
      },
      201,
      json_answer::<Synthesis>(&mut generator),
      &["unauthorized", "bad-request", "not-found", "unknown-emotion"],
    ),
  );
  path(
    "/syntheses/{synthesisId}",
    "get",
    operation(
      "readSynthesis",
      "syntheses",
      "One synthesis and its state.",
      Asked {
        parameters: vec![synthesis_key()],
        body: None,
      },
      200,
      json_answer::<Synthesis>(&mut generator),
      &["unauthorized", "not-found"],
    ),
  );
  path(
    "/syntheses/{synthesisId}/sound",
    "get",
    operation(
      "readSynthesisSound",
      "syntheses",
      "A synthesis's sound, OGG/Opus, with Range.",
      Asked {
        parameters: vec![synthesis_key()],
        body: None,
      },
      200,
      json!({
        "application/octet-stream": {
          "schema": { "type": "string", "contentEncoding": "binary" },
        },
      }),
      &["unauthorized", "not-found"],
    ),
  );
  path(
    "/transcriptions",
    "post",
    operation(
      "createTranscription",
      "transcriptions",
      "Transcribe sound files in a language.",
      Asked {
        parameters: Vec::new(),
        body: Some(generator.subschema_for::<NewTranscription>().to_value()),
      },
      201,
      json_answer::<Transcription>(&mut generator),
      &["unauthorized", "bad-request"],
    ),
  );
  path(
    "/transcriptions/{transcriptionId}",
    "get",
    operation(
      "readTranscription",
      "transcriptions",
      "One transcription and its words.",
      Asked {
        parameters: vec![transcription_key()],
        body: None,
      },
      200,
      json_answer::<Transcription>(&mut generator),
      &["unauthorized", "not-found"],
    ),
  );
  path(
    "/encodings",
    "post",
    operation(
      "createEncoding",
      "encodings",
      "Encode a reference recording into a speaker encoding.",
      Asked {
        parameters: Vec::new(),
        body: Some(generator.subschema_for::<NewEncoding>().to_value()),
      },
      201,
      json_answer::<Encoding>(&mut generator),
      &["unauthorized", "bad-request", "unreadable-source"],
    ),
  );
  path(
    "/script-readings",
    "post",
    operation(
      "createScriptReading",
      "script-readings",
      "Read a text into a script for the ear.",
      Asked {
        parameters: Vec::new(),
        body: Some(generator.subschema_for::<NewScriptReading>().to_value()),
      },
      200,
      json_answer::<ScriptReading>(&mut generator),
      &["unauthorized", "bad-request"],
    ),
  );
  path(
    "/voice-servers",
    "get",
    operation(
      "listVoiceServers",
      "voice-servers",
      "The voice servers on the nodes and the voices each has loaded.",
      Asked {
        parameters: page_params("The last node's name; the page starts past it."),
        body: None,
      },
      200,
      json_answer::<Page<VoiceServer>>(&mut generator),
      &["unauthorized", "bad-request"],
    ),
  );

  let x_tools: Vec<Value> = tools()
    .iter()
    .map(|tool| {
      json!({
        "name": tool.name,
        "description": tool.description,
        "input": tool.input(&mut generator),
        "output": tool.output(&mut generator),
        "route": tool.route,
      })
    })
    .collect();
  let responses = json!({
    "Error": {
      "description": "A failure, in the common error shape.",
      "content": { "application/json": { "schema": error } },
    },
  });

  json!({
    "openapi": "3.1.0",
    "info": {
      "title": "bmo-voice",
      "version": env!("CARGO_PKG_VERSION"),
      "description": DESCRIPTION,
    },
    "servers": [{ "url": "/", "description": "the voice service" }],
    "security": [{ "bearer": [] }],
    "x-layer": "upstream",
    "x-stage": 4,
    "x-calls": ["arbiter"],
    "x-harness": { "plugin": PLUGIN, "mcp": MCP, "entry": ENTRY },
    "tags": [
      { "name": "health" },
      { "name": "events" },
      { "name": "voices" },
      { "name": "syntheses" },
      { "name": "transcriptions" },
      { "name": "encodings" },
      { "name": "script-readings" },
      { "name": "voice-servers" },
    ],
    "paths": paths,
    "x-events": KINDS,
    "x-tools": x_tools,
    "x-moved-out": [
      { "route": "GET /events", "to": "briefing", "why": "The old stream was the listeners' briefing stream; the engine's own stream is new." },
      { "route": "GET /briefings", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "POST /briefings", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "GET /briefings/{briefingId}", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "PATCH /briefings/{briefingId}", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "POST /briefings/{briefingId}/lines", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "GET /briefings/{briefingId}/sound", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "GET /settings", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
      { "route": "PATCH /settings", "to": "briefing", "why": "Briefings, their rooms, listeners and switches are BMO's; the briefing service calls this engine." },
    ],
    "components": {
      "schemas": generator.take_definitions(true),
      "responses": responses,
      "securitySchemes": {
        "bearer": {
          "type": "http",
          "scheme": "bearer",
          "description": "The service's token, from the caller's own configuration.",
        },
      },
    },
  })
}

/// The document as the repository keeps it: two-space indented JSON with a closing newline.
pub fn openapi_json() -> String {
  let mut text = serde_json::to_string_pretty(&openapi()).unwrap_or_default();

  text.push('\n');
  text
}
