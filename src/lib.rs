//! The ontologic engine's API, shared by the engine, the CLI and the bench.
//!
//! - [`pb`]: the gRPC messages and service, generated from `proto/ontologic.proto`.
//! - [`client`] (feature `client`): a client that signs in on every call.
//! - [`fake`] (feature `fake`): a fake engine with scripted replies, for tests of clients.

/// The messages and the `Ontologic` service.
pub mod pb {
    // The import stream's events differ in size (a part's `Linked` carries far more than
    // `Finished`); they are sent one at a time, so boxing them would buy nothing.
    #![allow(clippy::large_enum_variant)]
    tonic::include_proto!("ontologic.v1");
}

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "fake")]
pub mod fake;

/// The engine's port when a host names none.
pub const DEFAULT_PORT: u16 = 6969;

/// The metadata every call signs in with: a user name and that user's key.
pub const USER_HEADER: &str = "user";
pub const KEY_HEADER: &str = "key";

/// What the engine answers when `user` and `key` do not belong to one user.
pub const SIGN_IN_FAILED: &str = "sign in failed: wrong user or key";

#[cfg(test)]
mod set_page_tests {
    use super::pb;
    use prost::Message;
    #[test]
    fn legacy_requests_do_not_opt_into_members_and_new_pages_round_trip() {
        // An old request encoded with only fields 1 and 2 has no new-field requirement.
        let request = pb::AskRequest::decode(&b"\x0a\x01t\x12\x01q"[..]).unwrap();
        assert!(!request.include_set_members);
        let page = pb::AskEvent {
            event: Some(pb::ask_event::Event::SetPage(pb::SetPage {
                result_id: "result1".into(),
                snapshot_id: "snapshot1".into(),
                last: true,
                member_count: 1,
                members: vec![pb::SetMember {
                    id: "record1".into(),
                    label: "Example".into(),
                    support: Vec::new(),
                }],
                count: Some(pb::CountRange {
                    operation: "count".into(),
                    lower: 1,
                    upper: Some(1),
                    status: "exact".into(),
                }),
                ..Default::default()
            })),
        };
        assert_eq!(
            pb::AskEvent::decode(page.encode_to_vec().as_slice()).unwrap(),
            page
        );
    }
}

#[cfg(test)]
mod ask_log_tests {
    use super::pb;
    use prost::Message;

    /// `AskAnswer` as v0.2.1 knew it: fields 1 to 6, nothing of the question log.
    #[derive(Clone, PartialEq, prost::Message)]
    struct OldAskAnswer {
        #[prost(string, tag = "1")]
        error: String,
        #[prost(message, repeated, tag = "2")]
        options: Vec<pb::AnswerOption>,
        #[prost(float, tag = "3")]
        something_else: f32,
        #[prost(string, repeated, tag = "4")]
        notes: Vec<String>,
        #[prost(bool, tag = "5")]
        exclusive: bool,
        #[prost(message, optional, tag = "6")]
        count: Option<pb::CountRange>,
    }

    /// `Finished` as v0.2.1 knew it, with no ask id.
    #[derive(Clone, PartialEq, prost::Message)]
    struct OldFinished {
        #[prost(message, optional, tag = "1")]
        cost: Option<pb::Cost>,
        #[prost(uint32, tag = "2")]
        waiting: u32,
        #[prost(message, optional, tag = "3")]
        timing: Option<pb::Timing>,
    }

    #[test]
    fn an_older_engine_reads_as_answered_with_no_id_and_an_older_client_skips_the_new_fields() {
        // An engine before 0.3.0 sends no status and no ask id: a 0.3.0 client reads them empty.
        let old = OldAskAnswer {
            notes: vec!["checked".into()],
            exclusive: true,
            ..Default::default()
        };
        let read = pb::AskAnswer::decode(old.encode_to_vec().as_slice()).unwrap();
        assert_eq!(
            (read.status.as_str(), read.status_reason.as_str()),
            ("", "")
        );
        assert_eq!(read.notes, ["checked"]);
        let old = OldFinished {
            waiting: 2,
            ..Default::default()
        };
        let finished = pb::Finished::decode(old.encode_to_vec().as_slice()).unwrap();
        assert_eq!((finished.ask_id.as_str(), finished.waiting), ("", 2));

        // A 0.3.0 engine's answer and Finished decode in a v0.2.1 client, which skips what it
        // does not know.
        let new = pb::AskAnswer {
            notes: vec!["unsure: the answer cites another subject".into()],
            exclusive: true,
            status: "unsure".into(),
            status_reason: "the answer cites another subject".into(),
            ..Default::default()
        };
        let seen = OldAskAnswer::decode(new.encode_to_vec().as_slice()).unwrap();
        assert_eq!(seen.notes, new.notes);
        assert!(seen.exclusive);
        let new = pb::Finished {
            waiting: 1,
            ask_id: "a1b2".into(),
            ..Default::default()
        };
        assert_eq!(
            OldFinished::decode(new.encode_to_vec().as_slice())
                .unwrap()
                .waiting,
            1
        );
    }
}
