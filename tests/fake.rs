//! The client against the fake engine: signing in, scripted replies and streams, and the calls
//! the fake remembers.

#![cfg(feature = "fake")]

use ontologic_kit::client::{connect, describe};
use ontologic_kit::fake::{FakeEngine, Rpc};
use ontologic_kit::{SIGN_IN_FAILED, pb};
use tonic::{Code, Status};

fn trusted(engine: &FakeEngine, name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ontologic-kit-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ca = dir.join("engine.pem");
    std::fs::write(&ca, engine.ca_pem()).unwrap();
    ca
}

#[tokio::test]
async fn a_client_signs_in_and_gets_the_scripted_replies_in_order() {
    let engine = FakeEngine::start();
    engine.sign_in("admin", "secret");
    engine.reply(
        Rpc::Health,
        pb::HealthReply {
            llm: "first".into(),
            ..Default::default()
        },
    );
    engine.reply(
        Rpc::Health,
        pb::HealthReply {
            llm: "then always".into(),
            ..Default::default()
        },
    );
    let ca = trusted(&engine, "order");
    let (mut client, host) = connect(engine.host(), Some(&ca), "admin", "secret").unwrap();
    assert_eq!(host, engine.host());
    let llm = |r: tonic::Response<pb::HealthReply>| r.into_inner().llm;
    assert_eq!(llm(client.health(pb::Empty {}).await.unwrap()), "first");
    assert_eq!(
        llm(client.health(pb::Empty {}).await.unwrap()),
        "then always"
    );
    assert_eq!(
        llm(client.health(pb::Empty {}).await.unwrap()),
        "then always"
    );
    let calls = engine.calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        (calls[0].rpc, calls[0].user.as_str(), calls[0].key.as_str()),
        (Rpc::Health, "admin", "secret")
    );
}

#[tokio::test]
async fn a_wrong_key_is_refused_with_the_engines_message() {
    let engine = FakeEngine::start();
    engine.sign_in("admin", "secret");
    engine.reply(Rpc::Me, pb::User::default());
    let ca = trusted(&engine, "key");
    let (mut client, _) = connect(engine.host(), Some(&ca), "admin", "wrong").unwrap();
    let status = client.me(pb::Empty {}).await.unwrap_err();
    assert_eq!(status.code(), Code::Unauthenticated);
    assert_eq!(status.message(), SIGN_IN_FAILED);
}

#[tokio::test]
async fn a_stream_sends_its_events_then_its_error_and_requests_are_kept() {
    let engine = FakeEngine::start();
    engine.stream(
        Rpc::Import,
        vec![
            Ok(pb::ImportEvent {
                event: Some(pb::import_event::Event::Source(pb::SourceSaved {
                    id: "s1".into(),
                    ..Default::default()
                })),
            }),
            Err(Status::failed_precondition(
                "an import is running in tenant acme",
            )),
        ],
    );
    let ca = trusted(&engine, "stream");
    let (mut client, _) = connect(engine.host(), Some(&ca), "admin", "secret").unwrap();
    let mut events = client
        .import(pb::ImportRequest {
            tenant: "acme".into(),
            text: "Maya lives in Toronto".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let first = events.message().await.unwrap().unwrap();
    assert!(matches!(first.event, Some(pb::import_event::Event::Source(s)) if s.id == "s1"));
    let error = events.message().await.unwrap_err();
    assert_eq!(error.message(), "an import is running in tenant acme");
    let asked: pb::ImportRequest = engine.calls()[0].request();
    assert_eq!(asked.text, "Maya lives in Toronto");
}

#[tokio::test]
async fn a_call_with_no_script_is_unimplemented_and_an_engine_that_is_gone_is_described() {
    let engine = FakeEngine::start();
    let ca = trusted(&engine, "none");
    let (mut client, _) = connect(engine.host(), Some(&ca), "admin", "secret").unwrap();
    let status = client.list_tenants(pb::Empty {}).await.unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);
    // The new calls, unscripted, answer UNIMPLEMENTED as every other call does here. This is the
    // code an engine built before 0.3.0 answers them with (tonic has no route for them), so a
    // client can tell "no question log" from a refusal; the fake still serves the route itself.
    let status = client
        .feedback(pb::FeedbackRequest::default())
        .await
        .unwrap_err();
    assert_eq!(status.code(), Code::Unimplemented);

    let (mut gone, host) = connect("localhost:1", Some(&ca), "admin", "secret").unwrap();
    let status = gone.health(pb::Empty {}).await.unwrap_err();
    assert!(
        describe(&status, &host)
            .starts_with("cannot reach engine at localhost:1 (is the engine running?)"),
        "{}",
        describe(&status, &host)
    );
}

#[tokio::test]
async fn the_lifecycle_and_access_calls_carry_what_they_are_given() {
    let engine = FakeEngine::start();
    engine.sign_in("admin", "secret");
    engine.reply(
        Rpc::Migrate,
        pb::MigrateReply {
            committed: false,
            diff: Some(pb::MigrationDiff {
                promoted: 3,
                model_calls: 0,
                ..Default::default()
            }),
        },
    );
    let ca = trusted(&engine, "lifecycle");
    let (mut client, _) = connect(engine.host(), Some(&ca), "admin", "secret").unwrap();
    let reply = client
        .migrate(pb::MigrateRequest {
            tenant: "acme".into(),
            kind: pb::MigrationKind::Reprojection as i32,
            pins: vec![pb::SchemaPin {
                kind: "ticket".into(),
                version: 10,
            }],
            reason: "rules".into(),
            commit: false,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(reply.diff.unwrap().promoted, 3);
    let asked: pb::MigrateRequest = engine.calls()[0].request();
    assert_eq!(asked.pins[0].version, 10);
}

/// An ask's `Finished` carries the id its question is kept under, and feedback and the log's days
/// go back with what they are given.
#[tokio::test]
async fn a_logged_answer_carries_its_id_and_feedback_names_it() {
    let engine = FakeEngine::start();
    engine.sign_in("maya", "secret");
    engine.stream(
        Rpc::Ask,
        vec![
            Ok(pb::AskEvent {
                event: Some(pb::ask_event::Event::Answer(pb::AskAnswer {
                    status: "unsure".into(),
                    status_reason: "the answer cites another subject".into(),
                    ..Default::default()
                })),
            }),
            Ok(pb::AskEvent {
                event: Some(pb::ask_event::Event::Finished(pb::Finished {
                    ask_id: "a1b2".into(),
                    ..Default::default()
                })),
            }),
        ],
    );
    engine.reply(Rpc::Feedback, pb::Empty {});
    engine.reply(Rpc::SetAskLog, pb::Empty {});
    let ca = trusted(&engine, "asklog");
    let (mut client, _) = connect(engine.host(), Some(&ca), "maya", "secret").unwrap();
    let mut events = client
        .ask(pb::AskRequest {
            tenant: "acme".into(),
            question: "who approved the pilot".into(),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_inner();
    let answer = events.message().await.unwrap().unwrap();
    assert!(matches!(answer.event, Some(pb::ask_event::Event::Answer(a)) if a.status == "unsure"));
    let finished = events.message().await.unwrap().unwrap();
    let Some(pb::ask_event::Event::Finished(f)) = finished.event else {
        panic!("no Finished")
    };
    client
        .feedback(pb::FeedbackRequest {
            tenant: "acme".into(),
            ask_id: f.ask_id,
            verdict: "wrong".into(),
            note: "it named the proposal, not the approval".into(),
        })
        .await
        .unwrap();
    client
        .set_ask_log(pb::AskLogRequest {
            tenant: "acme".into(),
            keep_days: 90,
        })
        .await
        .unwrap();
    let calls = engine.calls();
    let feedback: pb::FeedbackRequest = calls[1].request();
    assert_eq!(
        (feedback.ask_id.as_str(), feedback.verdict.as_str()),
        ("a1b2", "wrong")
    );
    let days: pb::AskLogRequest = calls[2].request();
    assert_eq!(days.keep_days, 90);
}

/// An export is a whole tenant's Turtle: a client reads one of several megabytes, where tonic's
/// default of 4 MiB refused a tenant of a few hundred documents.
#[tokio::test]
async fn a_client_reads_an_export_larger_than_four_megabytes() {
    let engine = FakeEngine::start();
    engine.sign_in("admin", "secret");
    let content = "brain:x brain:y brain:z .\n".repeat(250_000);
    assert!(content.len() > 6 << 20);
    engine.reply(
        Rpc::ExportTurtle,
        pb::TurtleFile {
            tenant: "acme".into(),
            content: content.clone(),
            summary: None,
        },
    );
    let ca = trusted(&engine, "large");
    let (mut client, _) = connect(engine.host(), Some(&ca), "admin", "secret").unwrap();
    let file = client
        .export_turtle(pb::TenantName {
            name: "acme".into(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(file.content.len(), content.len());
}
