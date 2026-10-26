//! Actual blocked source I/O tests responsiveness and snapshot truth.
//! [LSP-PULL-SALSA] [LSP-PULL-CONCURRENCY] [LSP-PULL-CANCELLATION]

use super::common::held_source::{release_original, source_session, HeldSource};
use super::common::{
    assert_partial_document, clean_exit, error_of, identity, partial_generation, report_identity,
    result_of, ReferenceClient, StepResult,
};
use lspkit_server::{JsonRpcError, Message};
use serde_json::{json, Value};

/// A primed good-source pull answers a clean full report with an identity.
fn assert_primed(response: &Message) {
    assert!(response.error.is_none());
    let report = result_of(response);
    assert_eq!(report.get("kind"), Some(&json!("full")));
    assert_eq!(report.get("items"), Some(&json!([])));
}

/// Prime the good source and extract its opaque previous identity.
async fn prime(
    client: &mut ReferenceClient,
    fixture: &HeldSource,
) -> StepResult<(Message, String)> {
    let response = client.pull_document(&fixture.good_uri).await?;
    let previous = result_of(&response)
        .get("resultId")
        .and_then(Value::as_str)
        .ok_or("real result identity")?
        .to_owned();
    Ok((response, previous))
}

/// Cancel an unknown request and pull the cached good-source truth.
async fn cached_pull(
    client: &mut ReferenceClient,
    fixture: &HeldSource,
    previous: &str,
) -> StepResult<(Message, Value)> {
    client
        .notify(
            "$/cancelRequest",
            json!({"id":"unknown-native-source-request"}),
        )
        .await?;
    let response = client.pull_previous(&fixture.good_uri, previous).await?;
    let work = client.work().await?;
    Ok((response, work))
}

/// Cached truth answers unchanged without rerunning the compiler.
fn assert_cached(response: &Message, previous: &str, work: &Value) {
    assert!(response.error.is_none());
    let report = result_of(response);
    assert_eq!(report.get("kind"), Some(&json!("unchanged")));
    assert_eq!(report.get("resultId"), Some(&json!(previous)));
    assert_eq!(
        work.get("analysisRuns"),
        Some(&json!(2)),
        "cached truth never waits for or reruns a compiler"
    );
}

/// Apply the editor overlay and pair the metadata with the prior revision.
async fn overlay_step(
    client: &mut ReferenceClient,
    fixture: &HeldSource,
    previous: &str,
) -> StepResult<(Value, u64)> {
    client
        .change_document(&fixture.source_uri, 2, "{\"fixed\":true}")
        .await?;
    let work = client.work().await?;
    let (previous_revision, _, _) = identity(previous)?;
    Ok((work, previous_revision))
}

/// Current input is clean, versioned correctly and bound to its actual epoch.
fn assert_current_report(report: &Value, fields: &(u64, u64, u64), revision: u64) {
    let (actual, version, generation) = *fields;
    assert_eq!(report.get("kind"), Some(&json!("full")));
    assert_eq!(report.get("items"), Some(&json!([])));
    assert_eq!(version, 2);
    assert_eq!(actual, generation);
    assert!(actual >= revision);
}

/// The completed old compiler query is rejected rather than published as current.
fn assert_superseded(response: &Message) {
    assert!(
        response.result.is_none(),
        "a superseded compiler result is never successful"
    );
    match response.error.clone() {
        Some(failure) => {
            assert_eq!(failure.code, -32603);
            assert!(failure.message.contains("superseded"));
        }
        None => assert!(
            response.error.is_some(),
            "superseded answer carries an error: {response:?}"
        ),
    }
}

/// Already streamed truth stays intact, with one standard terminal cancellation.
fn assert_cancelled_scan(
    terminal: &Message,
    error: &JsonRpcError,
    partials: &[Value],
    work: &Value,
    fixture: &HeldSource,
) {
    assert!(
        terminal.result.is_none(),
        "cancellation cannot claim success"
    );
    assert_eq!(error.code, -32800);
    assert_eq!(partials.len(), 1, "only the completed source was streamed");
    let streamed = partials.first().and_then(|partial| partial.get(0));
    assert_eq!(
        streamed.and_then(|report| report.get("uri")),
        Some(&json!(fixture.good_uri))
    );
    assert_eq!(
        streamed.and_then(|report| report.get("items")),
        Some(&json!([]))
    );
    assert_eq!(work.get("analysisRuns"), Some(&json!(2)));
}

/// The next uncached query is a real compiler-completion barrier for the old work.
fn assert_no_late_partial(
    response: &Message,
    late: &[Value],
    fields: &(u64, u64, u64),
    work: &Value,
    revision: u64,
) {
    assert!(response.error.is_none());
    assert!(
        late.is_empty(),
        "the closed stream rejects late compiler output: {late:?}"
    );
    assert_current_report(&result_of(response), fields, revision);
    assert_eq!(work.get("analysisRuns"), Some(&json!(3)));
}

/// One stale wave report, `uri` compared as a whole value.
fn assert_wave_uri(partial: &Value, uri: &str) {
    assert_eq!(
        partial.get(0).and_then(|report| report.get("uri")).cloned(),
        Some(json!(uri))
    );
}

/// The stale backend never streams, and a complete new wave replaces it.
fn assert_retried_reports(
    response: &Message,
    partials: &[Value],
    fixture: &HeldSource,
    revision: u64,
    fields: &(u64, u64, u64),
) {
    assert!(
        response.error.is_none(),
        "superseded scans retry: {response:?}"
    );
    assert_eq!(result_of(response).get("items"), Some(&json!([])));
    match partials {
        [stale, retry, current] => {
            assert_stale_wave(stale, retry, current, fixture, revision, fields);
        }
        _ => assert_eq!(partials.len(), 3, "one old and two current reports"),
    }
}

/// The stale wave precedes a complete retry wave and the current report.
fn assert_stale_wave(
    stale: &Value,
    retry: &Value,
    current: &Value,
    fixture: &HeldSource,
    revision: u64,
    fields: &(u64, u64, u64),
) {
    assert_wave_uri(stale, &fixture.good_uri);
    assert_eq!(partial_generation(stale), Some(1));
    assert_wave_uri(retry, &fixture.good_uri);
    assert_eq!(partial_generation(retry), Some(revision));
    assert_eq!(
        retry.get(0).and_then(|report| report.get("items")).cloned(),
        Some(json!([]))
    );
    assert_wave_uri(current, &fixture.source_uri);
    if let Some(report) = current.get(0) {
        assert_current_report(report, fields, revision);
    }
}

#[tokio::test]
async fn cached_pulls_remain_responsive_during_native_source_reads() {
    let (mut fixture, mut client) = source_session!(1);
    let (primed, previous) = prime(&mut client, &fixture)
        .await
        .expect("prime the good source");
    assert_primed(&primed);
    let pending = client
        .send(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":fixture.source_uri}}),
        )
        .await
        .expect("pull the held source");
    let writer = fixture.writer().expect("hold the native source read");
    let (cached, work) = cached_pull(&mut client, &fixture, &previous)
        .await
        .expect("pull the cached truth");
    assert_cached(&cached, &previous, &work);
    let revision = apply_overlay(&mut client, &fixture, &previous)
        .await
        .expect("apply the editor overlay");
    release_original(writer).expect("supply the original broken source");
    let superseded = client
        .response(&pending)
        .await
        .expect("reply for the held pull");
    assert_superseded(&superseded);
    let response = client
        .pull_document(&fixture.source_uri)
        .await
        .expect("pull the current overlay");
    let fields = report_identity(&result_of(&response)).expect("current identity");
    assert_current_report(&result_of(&response), &fields, revision);
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(3)));
    clean_exit!(client);
    client.read_eof().await.expect("stdout reaches EOF");
}

/// Apply the overlay and return its post-invalidation revision.
async fn apply_overlay(
    client: &mut ReferenceClient,
    fixture: &HeldSource,
    previous: &str,
) -> StepResult<u64> {
    let (work, previous_revision) = overlay_step(client, fixture, previous).await?;
    let revision = work
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or("current input revision")?;
    if revision <= previous_revision {
        return Err("input invalidation did not wait for analysis".into());
    }
    if work.get("analysisRuns") != Some(&json!(2)) {
        return Err("updating inputs performs no eager analysis".into());
    }
    Ok(revision)
}

/// Cancellation is answered while real compiler I/O remains held by the client.
#[tokio::test]
async fn cancelled_native_scan_rejects_late_compiler_partials() {
    let (mut fixture, mut client) = source_session!(1);
    let (primed, previous) = prime(&mut client, &fixture)
        .await
        .expect("prime the good source");
    assert_primed(&primed);
    let token = json!("cancel-held-native-source");
    let id = client
        .start_scan(&token)
        .await
        .expect("start the held scan");
    let writer = fixture.writer().expect("hold the native source read");
    client
        .notify("$/cancelRequest", json!({ "id": id }))
        .await
        .expect("cancel the held scan");
    let (terminal, partials) = client
        .collect_stream(&id, &token)
        .await
        .expect("collect the cancelled scan");
    let work = client.work().await.expect("work metadata");
    let error = error_of(&terminal).expect("cancelled scan answers an error");
    assert_cancelled_scan(&terminal, &error, &partials, &work, &fixture);

    let (cached, work) = cached_pull(&mut client, &fixture, &previous)
        .await
        .expect("pull the cached truth");
    assert_cached(&cached, &previous, &work);
    let revision = apply_overlay(&mut client, &fixture, &previous)
        .await
        .expect("apply the editor overlay");
    release_original(writer).expect("supply the original broken source");
    let (response, late) = client
        .request_with_progress(
            "textDocument/diagnostic",
            json!({
                "textDocument":{"uri":fixture.source_uri,"version":2}
            }),
            &token,
        )
        .await
        .expect("pull through the closed stream");
    let fields = report_identity(&result_of(&response)).expect("current identity");
    let work = client.work().await.expect("work metadata");
    assert_no_late_partial(&response, &late, &fields, &work, revision);
    clean_exit!(client);
    client.read_eof().await.expect("stdout reaches EOF");
}

/// A real edit supersedes an actually blocked backend, then the scan retries.
#[tokio::test]
async fn invalidated_native_scan_retries_current_overlay() {
    let (mut fixture, mut client) = source_session!(1);
    let (primed, previous) = prime(&mut client, &fixture)
        .await
        .expect("prime the good source");
    assert_primed(&primed);
    let token = json!("invalidated-native-source");
    let id = client.start_scan(&token).await.expect("start the scan");
    let barrier = client
        .first_partial(&id, &token)
        .await
        .expect("first streamed partial");
    assert_partial_document(&barrier);
    let writer = fixture.writer().expect("hold the native source read");
    let revision = apply_overlay(&mut client, &fixture, &previous)
        .await
        .expect("apply the editor overlay");
    release_original(writer).expect("supply the original broken source");
    let (response, mut partials) = client
        .collect_stream(&id, &token)
        .await
        .expect("collect the retried scan");
    partials.insert(0, barrier);
    let current = partials
        .get(2)
        .and_then(|partial| partial.get(0))
        .expect("current report partial");
    let fields = report_identity(current).expect("current report identity");
    assert_retried_reports(&response, &partials, &fixture, revision, &fields);
    let work = client.work().await.expect("work metadata");
    assert_eq!(work.get("analysisRuns"), Some(&json!(4)));
    clean_exit!(client);
    client.read_eof().await.expect("stdout reaches EOF");
}
