//! Actual ingress replacement scopes and first interruption. [LSP-PULL-CANCELLATION]
use std::collections::HashMap;

use super::common::held_source::{release_original, source_session};
use super::common::{error_of, result_of, uri_for, workspace_session, ReferenceClient};
use lspkit_server::{Message, RequestId};
use serde_json::json;

fn document(id: i64, uri: &str, previous: &str) -> Message {
    Message::request(
        RequestId::Number(id),
        "textDocument/diagnostic",
        json!({
            "textDocument":{"uri":uri}, "previousResultId":previous
        }),
    )
}

async fn replies(client: &mut ReferenceClient, count: usize) -> HashMap<RequestId, Message> {
    let mut replies = HashMap::new();
    while replies.len() < count {
        let message = client.next_message().await;
        if let Some(id) = message.id.clone() {
            assert!(replies.insert(id, message).is_none(), "one terminal per id");
        }
    }
    replies
}

fn superseded(message: &Message) {
    assert!(
        message.result.is_none(),
        "old compiler truth cannot succeed"
    );
    let error = error_of(message);
    assert_eq!(error.code, -32802);
    assert_eq!(error.data, Some(json!({"retriggerRequest":true})));
}

fn full(message: &Message) {
    assert!(message.error.is_none());
    let report = result_of(message);
    assert_eq!(report["kind"], "full");
    assert!(report["resultId"].is_string());
    assert_eq!(report["items"], json!([]));
}

#[tokio::test]
async fn ingress_replaces_changed_interest_but_preserves_equal_input_and_other_documents() {
    let (workspace, mut client) = workspace_session(2).await;
    let uri = uri_for(workspace.path().join("scan-0000.json"));
    let other = uri_for(workspace.path().join("scan-0001.json"));
    client
        .write_batch(&[
            document(100, &uri, "old"),
            document(101, &uri, "new"),
            document(102, &uri, "new"),
            document(103, &other, "old"),
        ])
        .await;
    let responses = replies(&mut client, 4).await;
    superseded(&responses[&RequestId::Number(100)]);
    for id in [101, 102, 103] {
        full(&responses[&RequestId::Number(id)]);
    }
    assert_eq!(client.work().await["analysisRuns"], 2);
    client.assert_clean_exit().await;
}

fn initialize(id: i64) -> Message {
    Message::request(
        RequestId::Number(id),
        "initialize",
        json!({"capabilities":{}}),
    )
}

#[tokio::test]
async fn replacing_workspace_context_retires_every_document_interest() {
    let (workspace, mut client) = workspace_session(2).await;
    client
        .write_batch(&[
            document(
                110,
                &uri_for(workspace.path().join("scan-0000.json")),
                "same",
            ),
            document(
                111,
                &uri_for(workspace.path().join("scan-0001.json")),
                "same",
            ),
            initialize(112),
        ])
        .await;
    let responses = replies(&mut client, 3).await;
    superseded(&responses[&RequestId::Number(110)]);
    superseded(&responses[&RequestId::Number(111)]);
    assert!(responses[&RequestId::Number(112)].error.is_none());
    assert!(result_of(&responses[&RequestId::Number(112)])["capabilities"].is_object());
    client.assert_clean_exit().await;
}

#[tokio::test]
async fn earlier_client_cancellation_survives_context_supersession() {
    let (_workspace, mut client) = workspace_session(2).await;
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
        .await;
    let responses = replies(&mut client, 2).await;
    let cancelled = &responses[&RequestId::Number(120)];
    assert!(cancelled.result.is_none());
    assert_eq!(error_of(cancelled).code, -32800);
    assert!(responses[&RequestId::Number(121)].error.is_none());
    assert_eq!(client.work().await["analysisRuns"], 0);
    client.assert_clean_exit().await;
}

#[tokio::test]
async fn native_input_supersedes_repeated_identity_then_repairs_and_regresses() {
    let (mut fixture, mut client) = source_session(1).await;
    let pending = client
        .send(
            "textDocument/diagnostic",
            json!({
                "textDocument":{"uri":fixture.source_uri},"previousResultId":"same"
            }),
        )
        .await;
    let writer = fixture.writer();
    client
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({
                "changes":[{"uri":fixture.source_uri,"type":2}]
            }),
        )
        .await;
    assert_eq!(client.work().await["analysisRuns"], 1);
    release_original(writer);
    superseded(&client.response(&pending).await);
    repaired_then_broken(&mut client, &fixture.source_uri).await;
    client.assert_clean_exit().await;
}

async fn repaired_then_broken(client: &mut ReferenceClient, uri: &str) {
    for (version, text, count) in [(2, "{\"fixed\":true}", 0), (3, "{\"broken\":", 1)] {
        client.change_document(uri, version, text).await;
        let response = client.pull_document(uri).await;
        assert!(response.error.is_none());
        let report = result_of(&response);
        assert_eq!(report["kind"], "full");
        assert_eq!(
            report["items"].as_array().expect("diagnostics").len(),
            count
        );
        assert!(report["resultId"]
            .as_str()
            .expect("identity")
            .contains(&format!("|d:{version}|")));
    }
}

async fn cancelled_document(client: &mut ReferenceClient, params: serde_json::Value, runs: usize) {
    let id = client
        .send_cancelled("textDocument/diagnostic", params)
        .await;
    let response = client.response(&id).await;
    assert!(
        response.result.is_none(),
        "a cancelled document never claims success"
    );
    assert_eq!(error_of(&response).code, -32800);
    assert_eq!(client.work().await["analysisRuns"], runs);
}

#[tokio::test]
async fn cancelled_documents_never_run_analysis_or_replay_cached_success() {
    let (workspace, mut client) = workspace_session(1).await;
    let uri = uri_for(workspace.path().join("scan-0000.json"));
    cancelled_document(&mut client, json!({"textDocument":{"uri":uri}}), 0).await;
    let first = client.pull_document(&uri).await;
    full(&first);
    let previous = result_of(&first)["resultId"]
        .as_str()
        .expect("identity")
        .to_owned();
    cancelled_document(
        &mut client,
        json!({"textDocument":{"uri":uri},"previousResultId":previous}),
        1,
    )
    .await;
    let recovered = client.pull_previous(&uri, &previous).await;
    assert!(recovered.error.is_none());
    assert_eq!(result_of(&recovered)["kind"], "unchanged");
    assert_eq!(result_of(&recovered)["resultId"], previous);
    assert_eq!(client.work().await["analysisRuns"], 1);
    client.assert_clean_exit().await;
}

#[tokio::test]
async fn malformed_native_uris_return_errors_without_hiding_later_compiler_truth() {
    let (workspace, mut client) = workspace_session(1).await;
    for uri in ["file:///%GG.json", "file:///%0G.json", "file:///%ff.json"] {
        let response = client.pull_document(uri).await;
        assert!(
            response.result.is_none(),
            "invalid input cannot claim a clean report"
        );
        assert_eq!(error_of(&response).code, -32603);
        assert!(!error_of(&response).message.is_empty());
    }
    let uri = uri_for(workspace.path().join("scan-0000.json"));
    full(&client.pull_document(&uri).await);
    repaired_then_broken(&mut client, &uri).await;
    client.assert_clean_exit().await;
}
