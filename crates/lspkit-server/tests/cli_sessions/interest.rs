//! Actual ingress replacement scopes and first interruption. [LSP-PULL-CANCELLATION]
use std::collections::HashMap;

use super::common::held_source::{release_original, source_session};
use super::common::{
    clean_exit, error_of, result_of, uri_for, workspace_session, ReferenceClient, StepResult,
};
use lspkit_server::{Message, RequestId};
use serde_json::{json, Value};

/// One document pull request frame.
fn document(id: i64, uri: &str, previous: &str) -> Message {
    Message::request(
        RequestId::Number(id),
        "textDocument/diagnostic",
        json!({
            "textDocument":{"uri":uri}, "previousResultId":previous
        }),
    )
}

/// One initialize request frame.
fn initialize(id: i64) -> Message {
    Message::request(
        RequestId::Number(id),
        "initialize",
        json!({"capabilities":{}}),
    )
}

/// Collect `count` terminal replies keyed by id; a second reply for one id
/// fails the step, preserving the one-terminal-per-id contract.
async fn replies(
    client: &mut ReferenceClient,
    count: usize,
) -> StepResult<HashMap<RequestId, Message>> {
    let mut replies = HashMap::new();
    while replies.len() < count {
        let message = client.next_message().await?;
        if let Some(id) = message.id.clone() {
            if replies.insert(id, message).is_some() {
                return Err("one terminal per id".into());
            }
        }
    }
    Ok(replies)
}

/// An old compiler result is superseded, never published as current truth.
fn assert_superseded(message: &Message) {
    assert!(
        message.result.is_none(),
        "old compiler truth cannot succeed"
    );
    match message.error.clone() {
        Some(error) => {
            assert_eq!(error.code, -32802);
            assert_eq!(error.data, Some(json!({"retriggerRequest":true})));
        }
        None => assert!(
            message.error.is_some(),
            "superseded reply carries an error: {message:?}"
        ),
    }
}

/// A complete fresh report with no findings.
fn assert_full(message: &Message) {
    assert!(message.error.is_none());
    let report = result_of(message);
    assert_eq!(report.get("kind"), Some(&json!("full")));
    assert!(report.get("resultId").is_some_and(Value::is_string));
    assert_eq!(report.get("items"), Some(&json!([])));
}

#[tokio::test]
async fn ingress_replaces_changed_interest_but_preserves_equal_input_and_other_documents() {
    let (workspace, mut client) = workspace_session!(2);
    let uri = uri_for(workspace.path().join("scan-0000.json")).expect("fixture uri");
    let other = uri_for(workspace.path().join("scan-0001.json")).expect("fixture uri");
    client
        .write_batch(&[
            document(100, &uri, "old"),
            document(101, &uri, "new"),
            document(102, &uri, "new"),
            document(103, &other, "old"),
        ])
        .await
        .expect("batch the four document pulls");
    let responses = replies(&mut client, 4)
        .await
        .expect("four terminal replies");
    let replaced = responses
        .get(&RequestId::Number(100))
        .expect("terminal for id 100");
    assert_superseded(replaced);
    for id in [101, 102, 103] {
        let terminal = responses
            .get(&RequestId::Number(id))
            .expect("terminal reply for id");
        assert_full(terminal);
    }
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(2)));
    clean_exit!(client);
}

#[tokio::test]
async fn replacing_workspace_context_retires_every_document_interest() {
    let (workspace, mut client) = workspace_session!(2);
    let first = uri_for(workspace.path().join("scan-0000.json")).expect("fixture uri");
    let second = uri_for(workspace.path().join("scan-0001.json")).expect("fixture uri");
    client
        .write_batch(&[
            document(110, &first, "same"),
            document(111, &second, "same"),
            initialize(112),
        ])
        .await
        .expect("batch the context replacement");
    let responses = replies(&mut client, 3)
        .await
        .expect("three terminal replies");
    for id in [110, 111] {
        let terminal = responses
            .get(&RequestId::Number(id))
            .expect("terminal reply for id");
        assert_superseded(terminal);
    }
    let reinitialized = responses
        .get(&RequestId::Number(112))
        .expect("terminal for the new initialize");
    assert!(reinitialized.error.is_none());
    assert!(result_of(reinitialized)
        .get("capabilities")
        .is_some_and(Value::is_object));
    clean_exit!(client);
}

#[tokio::test]
async fn earlier_client_cancellation_survives_context_supersession() {
    let (_workspace, mut client) = workspace_session!(2);
    client
        .write_batch(&[
            Message::request(
                RequestId::Number(120),
                "workspace/diagnostic",
                json!({"previousResultIds":[]}),
            ),
            Message::notification("$/cancelRequest", json!({"id":120})),
            initialize(121),
        ])
        .await
        .expect("batch the cancelled scan and the new initialize");
    let responses = replies(&mut client, 2).await.expect("two terminal replies");
    let cancelled = responses
        .get(&RequestId::Number(120))
        .expect("terminal for the cancelled scan");
    assert!(cancelled.result.is_none());
    let error = error_of(cancelled).expect("cancellation answers an error");
    assert_eq!(error.code, -32800);
    let reinitialized = responses
        .get(&RequestId::Number(121))
        .expect("terminal for the new initialize");
    assert!(reinitialized.error.is_none());
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(0)));
    clean_exit!(client);
}

#[tokio::test]
async fn native_input_supersedes_repeated_identity_then_repairs_and_regresses() {
    let (mut fixture, mut client) = source_session!(1);
    let pending = client
        .send(
            "textDocument/diagnostic",
            json!({
                "textDocument":{"uri":fixture.source_uri},"previousResultId":"same"
            }),
        )
        .await
        .expect("pull with the repeated identity");
    let writer = fixture.writer().expect("hold the native source read");
    client
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({
                "changes":[{"uri":fixture.source_uri,"type":2}]
            }),
        )
        .await
        .expect("watcher change notification");
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(1)));
    release_original(writer).expect("supply the original broken source");
    let superseded = client
        .response(&pending)
        .await
        .expect("reply for the held pull");
    assert_superseded(&superseded);
    let responses = repaired_then_broken(&mut client, &fixture.source_uri)
        .await
        .expect("pulls after each edit");
    assert_repaired_then_broken(&responses);
    clean_exit!(client);
}

/// Apply both full-text edits and return their pull responses in order.
async fn repaired_then_broken(client: &mut ReferenceClient, uri: &str) -> StepResult<Vec<Message>> {
    let mut responses = Vec::new();
    for (version, text) in [(2, "{\"fixed\":true}"), (3, "{\"broken\":")] {
        client.change_document(uri, version, text).await?;
        responses.push(client.pull_document(uri).await?);
    }
    Ok(responses)
}

/// The repaired pull is clean and the regressed pull reports one finding.
fn assert_repaired_then_broken(responses: &[Message]) {
    assert_eq!(responses.len(), 2, "one pull per edited version");
    for (response, (version, count)) in responses.iter().zip([(2u64, 0usize), (3, 1)]) {
        assert!(response.error.is_none());
        let report = result_of(response);
        assert_eq!(report.get("kind"), Some(&json!("full")));
        assert_eq!(
            report
                .get("items")
                .and_then(Value::as_array)
                .map(std::vec::Vec::len),
            Some(count),
            "diagnostics count matches the edit"
        );
        assert_eq!(
            report
                .get("resultId")
                .and_then(Value::as_str)
                .map(|id| id.contains(&format!("|d:{version}|"))),
            Some(true),
            "identity carries its document version"
        );
    }
}

/// Send a document pull batched with its own cancellation and collect the reply.
async fn cancelled_document(client: &mut ReferenceClient, params: Value) -> StepResult<Message> {
    let id = client
        .send_cancelled("textDocument/diagnostic", params)
        .await?;
    client.response(&id).await
}

/// A cancelled document never claims success and never ran analysis.
fn assert_cancelled_document(response: &Message, work: &Value, runs: u64) {
    assert!(
        response.result.is_none(),
        "a cancelled document never claims success"
    );
    match response.error.clone() {
        Some(error) => assert_eq!(error.code, -32800),
        None => assert!(
            response.error.is_some(),
            "cancellation answers an error: {response:?}"
        ),
    }
    assert_eq!(work.get("analysisRuns"), Some(&json!(runs)));
}

#[tokio::test]
async fn cancelled_documents_never_run_analysis_or_replay_cached_success() {
    let (workspace, mut client) = workspace_session!(1);
    let uri = uri_for(workspace.path().join("scan-0000.json")).expect("fixture uri");
    let first_cancelled = cancelled_document(&mut client, json!({"textDocument":{"uri":uri}}))
        .await
        .expect("cancelled pull without a previous identity");
    let work = client.work().await.expect("work metadata");
    assert_cancelled_document(&first_cancelled, &work, 0);

    let first = client.pull_document(&uri).await.expect("first live pull");
    assert_full(&first);
    let previous = result_of(&first)
        .get("resultId")
        .and_then(Value::as_str)
        .expect("identity")
        .to_owned();
    let second_cancelled = cancelled_document(
        &mut client,
        json!({"textDocument":{"uri":uri},"previousResultId":previous}),
    )
    .await
    .expect("cancelled pull with the previous identity");
    let work = client.work().await.expect("work metadata");
    assert_cancelled_document(&second_cancelled, &work, 1);

    let recovered = client
        .pull_previous(&uri, &previous)
        .await
        .expect("recover with the previous identity");
    assert!(recovered.error.is_none());
    let report = result_of(&recovered);
    assert_eq!(report.get("kind"), Some(&json!("unchanged")));
    assert_eq!(report.get("resultId"), Some(&json!(previous)));
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(1)));
    clean_exit!(client);
}

#[tokio::test]
async fn malformed_native_uris_return_errors_without_hiding_later_compiler_truth() {
    let (workspace, mut client) = workspace_session!(1);
    for uri in ["file:///%GG.json", "file:///%0G.json", "file:///%ff.json"] {
        let response = client
            .pull_document(uri)
            .await
            .expect("pull the malformed uri");
        assert!(
            response.result.is_none(),
            "invalid input cannot claim a clean report"
        );
        let error = error_of(&response).expect("malformed uri answers an error");
        assert_eq!(error.code, -32603);
        assert_ne!(error.message, "");
    }
    let uri = uri_for(workspace.path().join("scan-0000.json")).expect("fixture uri");
    let live = client
        .pull_document(&uri)
        .await
        .expect("pull the live source");
    assert_full(&live);
    let responses = repaired_then_broken(&mut client, &uri)
        .await
        .expect("pulls after each edit");
    assert_repaired_then_broken(&responses);
    clean_exit!(client);
}
