//! `bmo-voice mcp`: the harness plugin's MCP server on standard input and output (`docs/boundary-api.md`,
//! 「The harness plugin」). Each tool is a thin call of one route through the service's own client; the server
//! lists them and the OpenAPI document states them under `x-tools`, both from the one table here.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::model::{
  CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, InitializeResult,
  JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::client::{ClientFailure, VoiceClient};
use crate::wire::{NewScriptReading, NewSynthesis, NewTranscription, PageQuery};

/// The plugin a harness takes the voice in, and the command that starts its MCP server.
pub const PLUGIN: &str = "bmo-voice";
pub const MCP: &str = "bmo-voice mcp";

/// What a harness does first with it, which the server tells a harness when it starts.
pub const ENTRY: &str = "A harness turns text into speech with voice_synthesize in a voice voice_voices lists, \
  and reads the sound when voice_synthesis says it is done.";

/// What a tool's call came to short of the route's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolFailure {
  /// The arguments are not the shape the tool takes.
  Arguments(String),
  /// The call reached no answer of the route's shape.
  Client(ClientFailure),
}

impl ToolFailure {
  /// The failure as the tool error's content: the error shape the service answered with and its status, or the
  /// sentence saying why nothing answered.
  pub fn content(&self) -> Value {
    match self {
      Self::Arguments(why) => json!({ "code": "bad-request", "error": why }),
      Self::Client(ClientFailure::Answered {
        status,
        answer: Some(answer),
        ..
      }) => json!({ "status": status, "code": answer.code, "error": answer.error }),
      Self::Client(failure @ ClientFailure::Answered { status, .. }) => {
        json!({ "status": status, "error": failure.to_string() })
      }
      Self::Client(failure) => json!({ "error": failure.to_string() }),
    }
  }
}

type Answering = Pin<Box<dyn Future<Output = Result<Value, ToolFailure>> + Send>>;
type Call = Box<dyn Fn(VoiceClient, Value) -> Answering + Send + Sync>;

/// One tool: its name, what it does, the route it calls, its input and output, and the call itself.
pub struct VoiceTool {
  pub name: &'static str,
  pub description: &'static str,
  /// The route the tool calls, as its method and path.
  pub route: &'static str,
  input: fn(&mut SchemaGenerator) -> Schema,
  output: fn(&mut SchemaGenerator) -> Schema,
  call: Call,
}

impl VoiceTool {
  /// The tool's input, its nested types referred to where `generator` keeps its definitions.
  pub fn input(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.input)(generator)
  }

  /// The tool's output, referred to where `generator` keeps its definitions.
  pub fn output(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.output)(generator)
  }

  /// Calls the tool's route through `client` with the tool's arguments.
  pub async fn call(&self, client: VoiceClient, arguments: Value) -> Result<Value, ToolFailure> {
    (self.call)(client, arguments).await
  }
}

/// A tool whose input is `I` and whose answer is `O`, calling `call` with the arguments read as `I`.
fn tool<I, O, F, Answer>(
  name: &'static str,
  description: &'static str,
  route: &'static str,
  call: F,
) -> VoiceTool
where
  I: DeserializeOwned + JsonSchema,
  O: Serialize + JsonSchema,
  F: Fn(VoiceClient, I) -> Answer + Send + Sync + 'static,
  Answer: Future<Output = Result<O, ClientFailure>> + Send + 'static,
{
  VoiceTool {
    name,
    description,
    route,
    input: I::json_schema,
    output: SchemaGenerator::subschema_for::<O>,
    call: Box::new(move |client, arguments| {
      let pending =
        serde_json::from_value::<I>(arguments)
          .map(|input| call(client, input))
          .map_err(|error| {
            ToolFailure::Arguments(format!("The arguments are not the shape this tool takes: {error}."))
          });

      Box::pin(async move {
        let answer = pending?.await.map_err(ToolFailure::Client)?;

        serde_json::to_value(answer).map_err(|why| ToolFailure::Arguments(why.to_string()))
      })
    }),
  }
}

/// A text to speak.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Speaking {
  voice_name: String,
  /// The written words: what people read of the line.
  text: String,
  /// The words the voice says, checked before they are spoken; derived from `text` where absent.
  #[serde(default)]
  spoken: Option<String>,
  /// The language the line is said in, `ko` or `en`; where absent, the one its written words are in (English where more words hold a Latin letter than Hangul), else the voice's own.
  #[serde(default)]
  language: Option<String>,
  #[serde(default)]
  emotion: Option<String>,
  #[serde(default)]
  style: Option<String>,
  #[serde(default)]
  seed: Option<i64>,
  /// The caller's key for the line: asked again while its synthesis has not failed, that synthesis answers.
  #[serde(default)]
  key: Option<String>,
}

/// One synthesis, by its id.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OneSynthesis {
  synthesis_id: i64,
}

/// Sound files to take down in one language.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Transcribing {
  language: String,
  files: Vec<String>,
}

/// The five tools (`docs/boundary-api.md`, 「The harness plugin」).
pub fn tools() -> Vec<VoiceTool> {
  vec![
    tool(
      "voice_voices",
      "The trained voices, each with its model and emotions.",
      "GET /voices",
      |client, asked: PageQuery| async move { client.voices(&asked).await },
    ),
    tool(
      "voice_synthesize",
      "Speak a text in a voice; the synthesis is answered at once and its sound follows.",
      "POST /syntheses",
      |client, asked: Speaking| async move {
        let synthesis = NewSynthesis {
          voice_name: asked.voice_name,
          text: asked.text,
          spoken: asked.spoken,
          language: asked.language,
          emotion: asked.emotion,
          style: asked.style,
          seed: asked.seed,
          key: asked.key,
          asked_by: None,
        };

        client.create_synthesis(&synthesis).await
      },
    ),
    tool(
      "voice_synthesis",
      "One synthesis and its state.",
      "GET /syntheses/{synthesisId}",
      |client, asked: OneSynthesis| async move { client.synthesis(asked.synthesis_id).await },
    ),
    tool(
      "voice_transcribe",
      "Transcribe sound files in a language.",
      "POST /transcriptions",
      |client, asked: Transcribing| async move {
        let transcription = NewTranscription {
          language: asked.language,
          files: asked.files,
        };

        client.create_transcription(&transcription).await
      },
    ),
    tool(
      "voice_script",
      "Read a text into a script for the ear.",
      "POST /script-readings",
      |client, asked: NewScriptReading| async move { client.read_script(&asked).await },
    ),
  ]
}

/// The generator every tool schema and the OpenAPI document are written with: JSON Schema 2020-12, read as a
/// caller sends it, definitions kept at `definitions_path`.
pub fn generator(definitions_path: &'static str) -> SchemaGenerator {
  SchemaSettings::draft2020_12()
    .for_deserialize()
    .with(|settings| {
      settings.definitions_path = definitions_path.into();
      settings.meta_schema = None;
    })
    .into_generator()
}

/// The server over the service the client reaches.
pub struct VoiceServer {
  client: VoiceClient,
  tools: Arc<Vec<VoiceTool>>,
}

impl VoiceServer {
  pub fn new(client: VoiceClient) -> Self {
    Self {
      client,
      tools: Arc::new(tools()),
    }
  }

  /// The tools as MCP lists them, each input and output a schema of its own with its definitions inside.
  pub fn listed(&self) -> Vec<Tool> {
    self.tools.iter().map(listed).collect()
  }
}

fn listed(tool: &VoiceTool) -> Tool {
  let mut generator = generator("/$defs");
  let input = tool.input(&mut generator);
  let output = tool.output(&mut generator);
  let definitions = generator.take_definitions(true);
  let mut listed = Tool::new(tool.name, tool.description, standing_alone(input, &definitions));

  listed.output_schema = Some(Arc::new(standing_alone(output, &definitions)));
  listed
}

/// A schema made to stand on its own: an object at its root, with every definition it refers to beside it.
fn standing_alone(schema: Schema, definitions: &Map<String, Value>) -> JsonObject {
  let mut object = match schema.to_value() {
    Value::Object(object) if object.contains_key("$ref") => {
      let mut wrapped = Map::new();

      wrapped.insert("allOf".into(), Value::Array(vec![Value::Object(object)]));
      wrapped.insert("type".into(), "object".into());
      wrapped
    }
    Value::Object(object) => object,
    _ => Map::from_iter([("type".to_owned(), Value::from("object"))]),
  };

  if !definitions.is_empty() {
    object.insert("$defs".into(), Value::Object(definitions.clone()));
  }

  object
}

impl ServerHandler for VoiceServer {
  fn get_info(&self) -> InitializeResult {
    let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
    let mut server = Implementation::default();

    server.name = "bmo-voice".to_owned();
    server.version = crate::VERSION.to_owned();
    info.server_info = server;
    info.instructions = Some(ENTRY.to_owned());
    info
  }

  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    Ok(ListToolsResult::with_all_items(self.listed()))
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    _: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    let Some(tool) = self.tools.iter().find(|tool| tool.name == request.name) else {
      return Err(ErrorData::invalid_params(format!("There is no tool {}.", request.name), None));
    };
    let arguments = Value::Object(request.arguments.unwrap_or_default());
    let result = match tool.call(self.client.clone(), arguments).await {
      Ok(answer) => CallToolResult::structured(answer),
      Err(failure) => CallToolResult::error(vec![ContentBlock::text(failure.content().to_string())]),
    };

    Ok(CallToolResponse::Complete(result))
  }
}

/// The MCP server, run on standard input and output until its harness closes them.
pub async fn serve(client: VoiceClient) -> Result<(), String> {
  let running = rmcp::ServiceExt::serve(VoiceServer::new(client), rmcp::transport::stdio())
    .await
    .map_err(|why| format!("the MCP session did not start: {why}"))?;

  running
    .waiting()
    .await
    .map(|_| ())
    .map_err(|why| format!("the MCP session ended in a failure: {why}"))
}
