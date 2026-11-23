//! The operations the webview may ask for, and how each one becomes a GraphQL request.
//!
//! The webview sends an operation *name* and *arguments*. It never sends a document, a URL or a
//! header: every document below is fixed here, and the arguments are checked against a typed
//! struct per operation (unknown fields are refused) before they become GraphQL variables.
//! Nothing in this file touches the network or Tauri.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::error::{ApiError, ApiErrorKind};

/// Largest argument set accepted, as JSON text. A request body is Markdown, so it is generous.
const MAX_ARGS_BYTES: usize = 2 * 1024 * 1024;

/// Fields of a request, as the server's `GqlRequest` has them.
const REQUEST_FIELDS: &str = "id version title body bodyMissing project kind tags protocol change lane createdMs updatedMs";

/// The allowlist. The names are the server's GraphQL field names. `ALL` and `name` must agree
/// with `OPERATION_NAMES` in `app/utils/api/operations.ts` (a test in the client checks it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    ServerInfo,
    Requests,
    Request,
    RequestDetail,
    Artifact,
    Conversation,
    Notifications,
    Projects,
    Settings,
    AgentMcpConfig,
    SaveDraft,
    SubmitRequest,
    CheckSubmitRequest,
    SubmitRequests,
    DecideApproval,
    AnswerQuestion,
    CreateProject,
    UpdateSettings,
    RegisterConversation,
}

/// What a successful answer looks like, checked before it goes to the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Object,
    Array,
    String,
    Bool,
    /// A string, or `null` when there is nothing (a request without a registered conversation).
    OptionalString,
}

impl Shape {
    pub fn accepts(self, value: &Value) -> bool {
        match self {
            Shape::Object => value.is_object(),
            Shape::Array => value.is_array(),
            Shape::String => value.is_string(),
            Shape::Bool => value.is_boolean(),
            Shape::OptionalString => value.is_string() || value.is_null(),
        }
    }
}

/// A request ready to send: the fixed document and the checked variables.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub operation: Operation,
    pub document: String,
    pub variables: Value,
}

impl Prepared {
    /// The body of `POST /graphql`.
    pub fn body(&self) -> Value {
        json!({ "query": self.document, "variables": self.variables })
    }
}

impl Operation {
    pub const ALL: [Operation; 19] = [
        Operation::ServerInfo,
        Operation::Requests,
        Operation::Request,
        Operation::RequestDetail,
        Operation::Artifact,
        Operation::Conversation,
        Operation::Notifications,
        Operation::Projects,
        Operation::Settings,
        Operation::AgentMcpConfig,
        Operation::SaveDraft,
        Operation::SubmitRequest,
        Operation::CheckSubmitRequest,
        Operation::SubmitRequests,
        Operation::DecideApproval,
        Operation::AnswerQuestion,
        Operation::CreateProject,
        Operation::UpdateSettings,
        Operation::RegisterConversation,
    ];

    /// The name the webview uses, which is also the GraphQL field.
    pub fn name(self) -> &'static str {
        match self {
            Operation::ServerInfo => "serverInfo",
            Operation::Requests => "requests",
            Operation::Request => "request",
            Operation::RequestDetail => "requestDetail",
            Operation::Artifact => "artifact",
            Operation::Conversation => "conversation",
            Operation::Notifications => "notifications",
            Operation::Projects => "projects",
            Operation::Settings => "settings",
            Operation::AgentMcpConfig => "agentMcpConfig",
            Operation::SaveDraft => "saveDraft",
            Operation::SubmitRequest => "submitRequest",
            Operation::CheckSubmitRequest => "checkSubmitRequest",
            Operation::SubmitRequests => "submitRequests",
            Operation::DecideApproval => "decideApproval",
            Operation::AnswerQuestion => "answerQuestion",
            Operation::CreateProject => "createProject",
            Operation::UpdateSettings => "updateSettings",
            Operation::RegisterConversation => "registerConversation",
        }
    }

    pub fn parse(name: &str) -> Result<Operation, ApiError> {
        Operation::ALL
            .into_iter()
            .find(|op| op.name() == name)
            .ok_or_else(|| ApiError::new(ApiErrorKind::InvalidInput, "unknown operation"))
    }

    /// A mutation changes something on the server. It is never resent after an unclear outcome.
    pub fn is_mutation(self) -> bool {
        matches!(
            self,
            Operation::SaveDraft
                | Operation::SubmitRequest
                | Operation::SubmitRequests
                | Operation::DecideApproval
                | Operation::AnswerQuestion
                | Operation::CreateProject
                | Operation::UpdateSettings
                | Operation::RegisterConversation
        )
    }

    pub fn shape(self) -> Shape {
        match self {
            Operation::ServerInfo
            | Operation::Request
            | Operation::RequestDetail
            | Operation::Artifact
            | Operation::Settings
            | Operation::AgentMcpConfig
            | Operation::SaveDraft
            | Operation::SubmitRequest
            | Operation::CheckSubmitRequest
            | Operation::SubmitRequests
            | Operation::UpdateSettings => Shape::Object,
            Operation::Requests | Operation::Notifications | Operation::Projects | Operation::DecideApproval => {
                Shape::Array
            }
            Operation::CreateProject => Shape::String,
            Operation::Conversation => Shape::OptionalString,
            Operation::AnswerQuestion | Operation::RegisterConversation => Shape::Bool,
        }
    }

    fn document(self) -> String {
        let f = REQUEST_FIELDS;
        match self {
            Operation::ServerInfo => "query { serverInfo { version contentBackend contentSupported contentMissingTools contentDetail } }".to_owned(),
            Operation::Requests => "query($filter: RequestFilter) { requests(filter: $filter) { id version title project kind tags lane currentStep updatedMs } }".to_owned(),
            Operation::Request => format!("query($id: String!) {{ request(id: $id) {{ {f} }} }}"),
            Operation::RequestDetail => "query($id: String!) { requestDetail(id: $id) }".to_owned(),
            Operation::Artifact => "query($requestId: String!, $artifactRef: String!) { artifact(requestId: $requestId, artifactRef: $artifactRef) }".to_owned(),
            Operation::Conversation => "query($requestId: String!) { conversation(requestId: $requestId) }".to_owned(),
            Operation::Notifications => "query($limit: Int) { notifications(limit: $limit) }".to_owned(),
            Operation::Projects => "query { projects }".to_owned(),
            Operation::Settings => "query { settings }".to_owned(),
            Operation::AgentMcpConfig => "query { agentMcpConfig }".to_owned(),
            Operation::SaveDraft => format!("mutation($input: SaveDraftInput!) {{ saveDraft(input: $input) {{ {f} }} }}"),
            Operation::SubmitRequest => format!("mutation($id: String!, $version: Int!) {{ submitRequest(id: $id, version: $version) {{ {f} }} }}"),
            Operation::CheckSubmitRequest => "query($id: String!, $version: Int!) { checkSubmitRequest(id: $id, version: $version) { id ok code missing message } }".to_owned(),
            Operation::SubmitRequests => format!("mutation($items: [SubmitItemInput!]!) {{ submitRequests(items: $items) {{ ok results {{ id ok code missing message }} unreverted requests {{ {f} }} }} }}"),
            Operation::DecideApproval => "mutation($requestId: String!, $proposalRef: String!, $proposalVersion: Int!, $decision: JSON!) { decideApproval(requestId: $requestId, proposalRef: $proposalRef, proposalVersion: $proposalVersion, decision: $decision) }".to_owned(),
            Operation::AnswerQuestion => "mutation($requestId: String!, $questionId: String!, $choice: String!) { answerQuestion(requestId: $requestId, questionId: $questionId, choice: $choice) }".to_owned(),
            Operation::CreateProject => "mutation($name: String!) { createProject(name: $name) }".to_owned(),
            Operation::UpdateSettings => "mutation($settings: JSON!) { updateSettings(settings: $settings) }".to_owned(),
            Operation::RegisterConversation => "mutation($requestId: String!, $conversationId: String!) { registerConversation(requestId: $requestId, conversationId: $conversationId) }".to_owned(),
        }
    }

    /// Checks the name and the arguments and builds the request. Nothing is sent here.
    pub fn prepare(name: &str, args: Option<Value>) -> Result<Prepared, ApiError> {
        let operation = Operation::parse(name)?;
        let args = args.unwrap_or(Value::Null);
        if args.to_string().len() > MAX_ARGS_BYTES {
            return Err(ApiError::new(ApiErrorKind::InvalidInput, "the arguments are too large"));
        }
        let variables = operation.variables(args)?;
        Ok(Prepared { operation, document: operation.document(), variables })
    }

    fn variables(self, args: Value) -> Result<Value, ApiError> {
        match self {
            Operation::ServerInfo | Operation::Projects | Operation::Settings | Operation::AgentMcpConfig => {
                let Empty {} = parse(self, args)?;
                Ok(json!({}))
            }
            Operation::Requests => {
                let RequestsArgs { filter } = parse(self, args)?;
                Ok(json!({ "filter": filter }))
            }
            Operation::Request | Operation::RequestDetail => {
                let IdArgs { id } = parse(self, args)?;
                Ok(json!({ "id": id }))
            }
            Operation::Artifact => {
                let ArtifactArgs { request_id, artifact_ref } = parse(self, args)?;
                Ok(json!({ "requestId": request_id, "artifactRef": artifact_ref }))
            }
            Operation::Conversation => {
                let RequestIdArgs { request_id } = parse(self, args)?;
                Ok(json!({ "requestId": request_id }))
            }
            Operation::Notifications => {
                let NotificationsArgs { limit } = parse(self, args)?;
                Ok(json!({ "limit": limit }))
            }
            Operation::SaveDraft => {
                let draft: SaveDraftArgs = parse(self, args)?;
                Ok(json!({ "input": draft }))
            }
            Operation::SubmitRequest => {
                let SubmitArgs { id, version } = parse(self, args)?;
                Ok(json!({ "id": id, "version": version }))
            }
            Operation::CheckSubmitRequest => {
                let SubmitArgs { id, version } = parse(self, args)?;
                Ok(json!({ "id": id, "version": version }))
            }
            Operation::SubmitRequests => {
                let SubmitBatchArgs { items } = parse(self, args)?;
                if items.is_empty() {
                    return Err(ApiError::new(ApiErrorKind::InvalidInput, "submitRequests: no items"));
                }
                Ok(json!({ "items": items }))
            }
            Operation::DecideApproval => {
                let DecideArgs { request_id, proposal_ref, proposal_version, decision } = parse(self, args)?;
                Ok(json!({
                    "requestId": request_id,
                    "proposalRef": proposal_ref,
                    "proposalVersion": proposal_version,
                    "decision": decision,
                }))
            }
            Operation::AnswerQuestion => {
                let AnswerArgs { request_id, question_id, choice } = parse(self, args)?;
                Ok(json!({ "requestId": request_id, "questionId": question_id, "choice": choice }))
            }
            Operation::CreateProject => {
                let CreateProjectArgs { name } = parse(self, args)?;
                Ok(json!({ "name": name }))
            }
            Operation::UpdateSettings => {
                let UpdateSettingsArgs { settings } = parse(self, args)?;
                Ok(json!({ "settings": settings }))
            }
            Operation::RegisterConversation => {
                let RegisterConversationArgs { request_id, conversation_id } = parse(self, args)?;
                Ok(json!({ "requestId": request_id, "conversationId": conversation_id }))
            }
        }
    }
}

/// Reads the arguments as the operation's struct. The reason names the operation and the
/// serde message (which names the field, never a value).
fn parse<T: for<'de> Deserialize<'de>>(operation: Operation, args: Value) -> Result<T, ApiError> {
    // `null` and "no arguments" both mean an empty set.
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args)
        .map_err(|e| ApiError::new(ApiErrorKind::InvalidInput, format!("{}: {e}", operation.name())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RequestIdArgs {
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ArtifactArgs {
    request_id: String,
    artifact_ref: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationsArgs {
    limit: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestsArgs {
    filter: Option<FilterArgs>,
}

/// `orderBy` is one of the server's names (`UPDATED_DESC` …); the server checks the value.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FilterArgs {
    project: Option<String>,
    kinds: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    order_by: Option<String>,
}

/// Every item may be left out: a draft may be incomplete, and a left-out item is "not changed".
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SaveDraftArgs {
    id: Option<String>,
    version: Option<i64>,
    /// Left out = "not changed". The webview leaves out what the user did not touch, and a body
    /// it could not read: an empty one would overwrite the saved one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<String>,
    /// The same for the managed items: left out (or `null`) = not changed, an empty value
    /// (`""`, `[]`) = cleared. A new request starts empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    change: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitArgs {
    id: String,
    version: i64,
}

/// One draft of a batch: the id and the version read last (the list carries it).
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SubmitItemArg {
    id: String,
    version: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitBatchArgs {
    items: Vec<SubmitItemArg>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum DecisionArg {
    /// An empty struct variant, so that serde also refuses extra fields next to `type`.
    Approve {},
    Revise { instruction: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DecideArgs {
    request_id: String,
    proposal_ref: String,
    proposal_version: i64,
    decision: DecisionArg,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AnswerArgs {
    request_id: String,
    question_id: String,
    choice: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProjectArgs {
    name: String,
}

/// The server's `Settings` (its JSON uses these snake_case names).
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LaneArg {
    name: String,
    label: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsArg {
    lanes: Vec<LaneArg>,
    kinds: Vec<String>,
    scheduler_interval_secs: u64,
    bantou_endpoint: Option<String>,
    memory_endpoint: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateSettingsArgs {
    settings: SettingsArg,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RegisterConversationArgs {
    request_id: String,
    conversation_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare(name: &str, args: Value) -> Result<Prepared, ApiError> {
        Operation::prepare(name, Some(args))
    }

    #[test]
    fn every_operation_has_a_distinct_name_that_parses_back() {
        let mut names: Vec<_> = Operation::ALL.iter().map(|op| op.name()).collect();
        for op in Operation::ALL {
            assert_eq!(Operation::parse(op.name()).unwrap(), op);
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Operation::ALL.len());
    }

    #[test]
    fn a_name_outside_the_list_is_refused() {
        for name in ["", "Requests", "requests ", "__schema", "query", "http://x", "requests { id }", "REQUESTS"] {
            let error = Operation::prepare(name, None).unwrap_err();
            assert_eq!(error.kind, ApiErrorKind::InvalidInput, "{name:?}");
        }
    }

    #[test]
    fn fields_that_would_steer_the_request_are_refused() {
        for extra in ["url", "headers", "query", "document", "variables", "method", "authorization"] {
            for op in ["serverInfo", "projects", "settings"] {
                let error = prepare(op, json!({ extra: "x" })).unwrap_err();
                assert_eq!(error.kind, ApiErrorKind::InvalidInput, "{op} {extra}");
            }
            let error = prepare("request", json!({ "id": "r1", extra: "x" })).unwrap_err();
            assert_eq!(error.kind, ApiErrorKind::InvalidInput, "request {extra}");
            let error = prepare("saveDraft", json!({ "title": "t", extra: "x" })).unwrap_err();
            assert_eq!(error.kind, ApiErrorKind::InvalidInput, "saveDraft {extra}");
        }
        // Nested objects are checked too.
        assert!(prepare("requests", json!({ "filter": { "project": "p", "url": "x" } })).is_err());
        assert!(prepare("decideApproval", json!({
            "requestId": "r", "proposalRef": "p", "proposalVersion": 1,
            "decision": { "type": "approve", "extra": 1 }
        })).is_err());
        assert!(prepare("updateSettings", json!({ "settings": {
            "lanes": [{ "name": "draft", "url": "x" }], "kinds": [], "scheduler_interval_secs": 5,
            "bantou_endpoint": null, "memory_endpoint": null
        }})).is_err());
    }

    #[test]
    fn arguments_of_the_wrong_type_are_refused() {
        assert!(prepare("request", json!({ "id": 5 })).is_err());
        assert!(prepare("request", json!({})).is_err());
        assert!(prepare("submitRequest", json!({ "id": "r", "version": "1" })).is_err());
        assert!(prepare("notifications", json!({ "limit": -1 })).is_err());
        assert!(prepare("saveDraft", json!({ "tags": "a" })).is_err());
        assert!(prepare("decideApproval", json!({
            "requestId": "r", "proposalRef": "p", "proposalVersion": 1, "decision": { "type": "delete" }
        })).is_err());
        assert!(Operation::prepare("serverInfo", Some(json!([1]))).is_err());
        let big = "x".repeat(MAX_ARGS_BYTES + 1);
        assert!(prepare("createProject", json!({ "name": big })).is_err());
    }

    #[test]
    fn the_document_is_fixed_and_the_arguments_become_variables() {
        let p = prepare("requests", json!({ "filter": { "project": "p1", "kinds": ["a"], "orderBy": "UPDATED_ASC" } })).unwrap();
        assert!(p.document.starts_with("query($filter: RequestFilter) { requests(filter: $filter)"));
        assert_eq!(p.variables, json!({ "filter": { "project": "p1", "kinds": ["a"], "tags": null, "orderBy": "UPDATED_ASC" } }));
        assert_eq!(p.body()["query"], p.document);

        // The text of an argument never reaches the document, whatever it contains.
        let hostile = "x\") { id } } mutation { createProject(name: \"y";
        let p = prepare("request", json!({ "id": hostile })).unwrap();
        assert!(!p.document.contains("createProject"));
        assert_eq!(p.variables, json!({ "id": hostile }));
    }

    #[test]
    fn a_draft_may_be_incomplete_and_an_update_carries_its_version() {
        let p = prepare("saveDraft", json!({})).unwrap();
        // Nothing said about the title and the body: neither is sent (an empty one would replace it).
        assert_eq!(
            p.variables,
            json!({ "input": { "id": null, "version": null } })
        );
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "title": "t", "tags": ["a"] })).unwrap();
        assert_eq!(p.variables["input"]["id"], "r1");
        assert_eq!(p.variables["input"]["version"], 3);
    }

    #[test]
    fn a_body_that_is_left_out_is_not_sent_and_one_that_is_given_is_sent_as_it_is() {
        // The saved body cannot be read, so an update that did not touch it must not carry one.
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "title": "t", "project": "p" })).unwrap();
        let input = p.variables["input"].as_object().unwrap();
        assert!(!input.contains_key("body"), "{input:?}");
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "body": "text" })).unwrap();
        assert_eq!(p.variables["input"]["body"], "text");
        assert!(!p.variables["input"].as_object().unwrap().contains_key("title"));
        // An empty body that the user wrote on purpose is a body: it is sent, not dropped.
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "body": "" })).unwrap();
        assert_eq!(p.variables["input"]["body"], "");
        // `null` is "not given" too (never a value that reaches the server as an empty text).
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "body": null })).unwrap();
        assert!(!p.variables["input"].as_object().unwrap().contains_key("body"));
    }

    #[test]
    fn an_emptied_item_is_sent_empty_and_one_left_out_is_not_sent() {
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "protocol": "", "tags": [], "project": "", "kind": "", "change": "" })).unwrap();
        assert_eq!(
            p.variables["input"],
            json!({ "id": "r1", "version": 3, "project": "", "kind": "", "tags": [], "protocol": "", "change": "" })
        );
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "kind": "bug" })).unwrap();
        assert_eq!(p.variables["input"], json!({ "id": "r1", "version": 3, "kind": "bug" }));
        // `null` is "not changed", as the server reads it: it is not turned into an empty value.
        let p = prepare("saveDraft", json!({ "id": "r1", "version": 3, "protocol": null, "tags": null })).unwrap();
        assert_eq!(p.variables["input"], json!({ "id": "r1", "version": 3 }));
    }

    #[test]
    fn the_documents_ask_for_what_the_screens_read_of_the_content() {
        let request = prepare("request", json!({ "id": "r" })).unwrap();
        assert!(request.document.contains("bodyMissing"));
        let save = prepare("saveDraft", json!({})).unwrap();
        assert!(save.document.contains("bodyMissing"));
        let info = prepare("serverInfo", json!({})).unwrap();
        for field in ["contentBackend", "contentSupported", "contentMissingTools", "contentDetail"] {
            assert!(info.document.contains(field), "{field}");
        }
    }

    #[test]
    fn decisions_are_the_two_the_server_knows() {
        let ok = prepare("decideApproval", json!({
            "requestId": "r", "proposalRef": "p", "proposalVersion": 2, "decision": { "type": "revise", "instruction": "again" }
        })).unwrap();
        assert_eq!(ok.variables["decision"], json!({ "type": "revise", "instruction": "again" }));
        assert!(ok.document.contains("decision: $decision"));
        let ok = prepare("decideApproval", json!({
            "requestId": "r", "proposalRef": "p", "proposalVersion": 2, "decision": { "type": "approve" }
        })).unwrap();
        assert_eq!(ok.variables["decision"], json!({ "type": "approve" }));
    }

    #[test]
    fn mutations_are_the_ones_that_change_the_server() {
        let mutations: Vec<_> = Operation::ALL.into_iter().filter(|op| op.is_mutation()).map(|op| op.name()).collect();
        assert_eq!(
            mutations,
            ["saveDraft", "submitRequest", "submitRequests", "decideApproval", "answerQuestion", "createProject", "updateSettings", "registerConversation"]
        );
        for op in Operation::ALL {
            assert_eq!(op.document().starts_with("mutation"), op.is_mutation(), "{}", op.name());
        }
    }

    #[test]
    fn the_check_and_the_batch_take_ids_with_versions_and_nothing_else() {
        let check = prepare("checkSubmitRequest", json!({ "id": "r1", "version": 3 })).unwrap();
        assert!(check.document.starts_with("query("));
        assert_eq!(check.variables, json!({ "id": "r1", "version": 3 }));
        assert!(prepare("checkSubmitRequest", json!({ "id": "r1" })).is_err());
        assert!(prepare("checkSubmitRequest", json!({ "id": "r1", "version": 3, "url": "x" })).is_err());

        let batch = prepare("submitRequests", json!({ "items": [{ "id": "a", "version": 1 }, { "id": "b", "version": 2 }] })).unwrap();
        assert!(batch.document.starts_with("mutation("));
        assert_eq!(batch.variables, json!({ "items": [{ "id": "a", "version": 1 }, { "id": "b", "version": 2 }] }));
        // Nothing to submit, a field that is not an id and a version, or a stray argument.
        assert!(prepare("submitRequests", json!({ "items": [] })).is_err());
        assert!(prepare("submitRequests", json!({})).is_err());
        assert!(prepare("submitRequests", json!({ "items": [{ "id": "a", "version": 1, "url": "x" }] })).is_err());
        assert!(prepare("submitRequests", json!({ "items": [{ "id": "a", "version": "1" }] })).is_err());
        assert!(prepare("submitRequests", json!({ "items": [{ "id": "a", "version": 1 }], "headers": {} })).is_err());
    }

    #[test]
    fn the_list_carries_the_version() {
        let p = prepare("requests", json!({})).unwrap();
        assert!(p.document.contains("{ id version title"));
    }

    #[test]
    fn no_arguments_are_needed_where_none_are_taken() {
        for name in ["serverInfo", "projects", "settings", "agentMcpConfig"] {
            assert!(Operation::prepare(name, None).is_ok());
            assert!(Operation::prepare(name, Some(Value::Null)).is_ok());
            assert!(Operation::prepare(name, Some(json!({}))).is_ok());
        }
        assert!(Operation::prepare("requests", None).is_ok());
        assert!(Operation::prepare("notifications", None).is_ok());
    }
}
