use companion_core::{
    authority::{Authority, Peer},
    client::Client,
    codec,
    model::Model,
    sync,
    wire::{envelope::Payload, *},
};
use std::collections::BTreeMap;
fn setup() -> (Client, Authority, Peer) {
    let body = b"printf 'synthetic approval fixture'\nNo actual command is executed.";
    let b = sync::blob(body).unwrap();
    let id = "claude:test";
    let request = "qa-1";
    let digest = companion_core::crypto::request_digest(
        request,
        id,
        1,
        "Run synthetic fixture",
        "Bash",
        &b.sha256.clone().try_into().unwrap(),
        b.byte_length,
        &[1, 2],
    )
    .unwrap()
    .to_vec();
    let s = Session {
        id: id.into(),
        title: "TEST CLAUDE".into(),
        provider: 1,
        status: 3,
        waiting_reason: 1,
        seat: 0,
        appearance_seed: 25,
        task_blob: Some(b.clone()),
        navigation_available: true,
        approval: Some(Request {
            id: request.into(),
            digest,
            title: "Run synthetic fixture".into(),
            tool_name: "Bash".into(),
            body_blob: Some(b),
            state: 1,
            allowed_decisions: vec![1, 2],
            ..Default::default()
        }),
        ..Default::default()
    };
    let model = Model {
        revision: 1,
        sessions: BTreeMap::from([(id.into(), s)]),
        view: View { page: 1, ..Default::default() },
        ..Default::default()
    };
    let mut c = Client::new("qa".into(), vec![1; 16], true, 0);
    c.outgoing();
    receive(
        &mut c,
        Payload::HelloAck(HelloAck {
            asset_digest: cafe_ui::ASSET_DIGEST.to_vec(),
            capabilities: vec![1, 2, 3, 4, 5, 6],
            required_capabilities: vec![1, 2, 6],
            heartbeat_ms: 1000,
            local_time_ms: Some(12 * 3_600_000 + 34 * 60_000),
            session_limit: 64,
            blob_limit: 32768,
            max_envelope_bytes: 4096,
            ..Default::default()
        }),
        0,
    );
    for e in sync::snapshot(&model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 0).unwrap();
    }
    for b in sync::chunks(body).unwrap() {
        c.blobs.receive(&b).unwrap();
    }
    // Advance the actual entrance animation; no test-only teleport of patrons.
    for t in (100..=10000).step_by(100) {
        c.tick(t).unwrap();
        while c.outgoing().is_some() {}
    }
    (c, Authority { model }, Peer::new("qa".into(), vec![1; 16], true, true))
}
fn receive(c: &mut Client, p: Payload, t: u64) {
    let e = sync::envelope(&[1; 16], &[2; 16], p);
    let raw = codec::encode(&e).unwrap();
    c.receive(e, &raw, t).unwrap();
}
fn tap(c: &mut Client, x: i32, y: i32, t: u64) {
    c.touch(1, x, y, t, &mut |_| true).unwrap();
    c.touch(0, x, y, t + 30, &mut |_| true).unwrap();
}
fn actions(c: &mut Client) -> Vec<Action> {
    std::iter::from_fn(|| c.outgoing())
        .filter_map(|e| if let Some(Payload::Action(a)) = e.payload { Some(a) } else { None })
        .collect()
}
fn open(c: &mut Client) {
    tap(c, 60, 90, 10010);
    tap(c, 200, 240, 10100);
    tap(c, 200, 240, 10300);
    assert_eq!(c.view().page, 2);
}
#[test]
fn immediate_selection_without_host_roundtrip_and_repeat_clear() {
    let (mut c, _, _) = setup();
    tap(&mut c, 60, 90, 10010);
    assert_eq!(c.scene.selected_id(), Some("claude:test"));
    assert_eq!(c.replica.model.view.selected_session_id, "");
    tap(&mut c, 60, 90, 11000);
    assert_eq!(c.scene.selected_id(), None);
    assert_eq!(actions(&mut c).len(), 2);
}
#[test]
fn double_tap_routes_stable_identity_without_second_toggle() {
    let (mut c, _, _) = setup();
    tap(&mut c, 60, 90, 10010);
    tap(&mut c, 60, 90, 10200);
    let aa = actions(&mut c);
    assert_eq!(aa.iter().map(|a| a.kind).collect::<Vec<_>>(), [1, 6]);
    assert_eq!(aa[1].session_id, "claude:test");
}
#[test]
fn approval_requires_present_then_quiet_new_contact_and_persist() {
    let (mut c, _, _) = setup();
    open(&mut c);
    let mut frame = vec![0; 480 * 272];
    c.render(&mut frame, 10350);
    c.presented(10350);
    tap(&mut c, 250, 240, 10400);
    assert!(actions(&mut c).iter().all(|a| a.kind != 7));
    c.tick(11000).unwrap();
    assert!(c.armed(11000));
    tap(&mut c, 250, 240, 11000);
    let aa = actions(&mut c);
    assert_eq!(aa.iter().filter(|a| a.kind == 7).count(), 1);
    assert_eq!(c.receipts.len(), 1);
    tap(&mut c, 250, 240, 12000);
    assert!(actions(&mut c).is_empty());
}
#[test]
fn journal_failure_never_sends_decision() {
    let (mut c, _, _) = setup();
    open(&mut c);
    let mut f = vec![0; 480 * 272];
    c.render(&mut f, 10350);
    c.presented(10350);
    c.tick(11000).unwrap();
    c.touch(1, 250, 240, 11000, &mut |_| false).unwrap();
    c.touch(0, 250, 240, 11030, &mut |_| false).unwrap();
    assert!(actions(&mut c).iter().all(|a| a.kind != 7));
    assert!(c.receipts.is_empty());
}
#[test]
fn sleep_wake_contact_is_consumed() {
    let (mut c, _, _) = setup();
    c.tick(610001).unwrap();
    assert!(c.sleeping);
    tap(&mut c, 60, 90, 610010);
    assert!(!c.sleeping);
    assert!(actions(&mut c).is_empty());
    tap(&mut c, 60, 90, 611000);
    assert_eq!(c.scene.selected_id(), Some("claude:test"));
}
#[test]
fn drag_and_multitouch_do_not_select() {
    let (mut c, _, _) = setup();
    c.touch(1, 60, 90, 10010, &mut |_| true).unwrap();
    c.touch(1, 100, 90, 10020, &mut |_| true).unwrap();
    c.touch(0, 60, 90, 10030, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty());
    c.touch(1, 60, 90, 10100, &mut |_| true).unwrap();
    c.touch(2, 60, 90, 10110, &mut |_| true).unwrap();
    c.touch(0, 60, 90, 10120, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty());
}
#[test]
fn result_before_delta_does_not_undo_optimistic_selection() {
    let (mut c, mut h, mut p) = setup();
    tap(&mut c, 60, 90, 10010);
    let a = actions(&mut c).pop().unwrap();
    let old = h.model.clone();
    let (r, _) = h.action(&mut p, &a, 10040, None).unwrap();
    receive(&mut c, Payload::ActionResult(r), 10050);
    assert_eq!(c.scene.selected_id(), Some("claude:test"));
    assert_eq!(c.intents.len(), 1);
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10060).unwrap();
    }
    assert!(c.intents.is_empty());
    assert_eq!(c.scene.selected_id(), Some("claude:test"));
}
#[test]
fn request_withdrawal_between_down_and_up_cannot_decide() {
    let (mut c, mut h, _) = setup();
    open(&mut c);
    let mut f = vec![0; 480 * 272];
    c.render(&mut f, 10350);
    c.presented(10350);
    c.tick(11000).unwrap();
    c.touch(1, 250, 240, 11000, &mut |_| true).unwrap();
    let old = h.model.clone();
    h.model.sessions.get_mut("claude:test").unwrap().approval = None;
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 11010).unwrap();
    }
    c.touch(0, 250, 240, 11030, &mut |_| true).unwrap();
    assert!(actions(&mut c).iter().all(|a| a.kind != 7));
}
#[test]
fn remote_open_requires_local_acknowledgement() {
    let (mut c, mut h, _) = setup();
    let old = h.model.clone();
    let r = h.model.sessions["claude:test"].approval.as_ref().unwrap();
    h.model.view = View {
        selected_session_id: "claude:test".into(),
        page: 2,
        review_kind: 1,
        review_request_id: r.id.clone(),
        review_blob: r.body_blob.clone(),
        ..Default::default()
    };
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10100).unwrap();
    }
    let mut f = vec![0; 480 * 272];
    c.render(&mut f, 10200);
    c.presented(10200);
    c.tick(11000).unwrap();
    assert!(!c.armed(11000));
    tap(&mut c, 250, 240, 11100);
    assert!(actions(&mut c).is_empty());
    tap(&mut c, 200, 130, 12000);
    c.render(&mut f, 12100);
    c.presented(12100);
    c.tick(13000).unwrap();
    assert!(c.armed(13000));
}
#[test]
fn corrupt_snapshot_cannot_publish_partial_sessions() {
    let (mut c, _, _) = setup();
    let old = c.replica.model.clone();
    receive(
        &mut c,
        Payload::SnapshotBegin(SnapshotBegin {
            revision: 2,
            session_count: 0,
            view: Some(View { page: 1, ..Default::default() }),
            ..Default::default()
        }),
        10100,
    );
    let e = sync::envelope(
        &[1; 16],
        &[2; 16],
        Payload::SnapshotCommit(SnapshotCommit { revision: 2, parts_count: 0, sha256: vec![0; 32] }),
    );
    let raw = codec::encode(&e).unwrap();
    assert!(c.receive(e, &raw, 10200).is_err());
    assert_eq!(c.replica.model, old);
    assert!(!c.connected);
}
#[test]
fn wrong_authority_disconnects_and_drops_queued_actions() {
    let (mut c, _, _) = setup();
    tap(&mut c, 60, 90, 10010);
    let e = sync::envelope(&[1; 16], &[3; 16], Payload::Heartbeat(Heartbeat::default()));
    let raw = codec::encode(&e).unwrap();
    assert!(c.receive(e, &raw, 10050).is_err());
    assert!(!c.connected);
    assert!(c.intents.is_empty());
    assert!(c.outgoing().is_none());
}
#[test]
fn duplicate_touch_after_decision_preserves_uncertain_receipt() {
    let (mut c, _, _) = setup();
    open(&mut c);
    let mut f = vec![0; 480 * 272];
    c.render(&mut f, 10350);
    c.presented(10350);
    c.tick(11000).unwrap();
    tap(&mut c, 250, 240, 11000);
    assert_eq!(c.receipts.len(), 1);
    c.tick(12000).unwrap();
    tap(&mut c, 250, 240, 12000);
    assert_eq!(c.receipts.len(), 1);
}
#[test]
fn rapidly_changing_task_keeps_completed_text_visible() {
    let (mut c, mut h, _) = setup();
    c.blobs = Default::default();
    let old = h.model.clone();
    h.model.sessions.get_mut("claude:test").unwrap().approval = None;
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10001).unwrap();
    }
    tap(&mut c, 60, 90, 10010);
    let before = h.model.clone();
    h.model.sessions.get_mut("claude:test").unwrap().task_blob = Some(sync::blob(b"newer body").unwrap());
    h.model.revision += 1;
    for e in sync::update(&before, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10100).unwrap();
    }
    for b in sync::chunks(b"printf 'synthetic approval fixture'\nNo actual command is executed.").unwrap() {
        receive(&mut c, Payload::BlobChunk(b), 10200);
    }
    assert!(c.scene.details()[0].task.contains("synthetic approval fixture"));
    while c.outgoing().is_some() {}
    c.tick(15300).unwrap();
    assert!(std::iter::from_fn(|| c.outgoing()).any(|e| matches!(e.payload, Some(Payload::BlobGet(_)))));
}

#[test]
fn pressed_overlay_is_local_and_never_arms_approval() {
    let (mut c, _, _) = setup();
    assert!(!c.feedback(|_, _| panic!("no contact")));
    c.touch(1, 60, 90, 10010, &mut |_| true).unwrap();
    let mut pixels = vec![];
    assert!(c.feedback(|x, y| pixels.push((x, y))));
    assert!(!pixels.is_empty());
    assert!(!c.armed(11000));
    c.touch(2, 60, 90, 10020, &mut |_| true).unwrap();
    assert!(!c.feedback(|_, _| panic!("cancelled contact")));
}

#[test]
fn host_clock_sync_startup_rollover_and_correction() {
    use chrono::NaiveTime;
    let (mut c, _, _) = setup();
    assert_eq!(c.local_time(0), NaiveTime::from_hms_opt(12, 34, 0).unwrap());
    assert_eq!(c.local_time(65_000), NaiveTime::from_hms_opt(12, 35, 5).unwrap());
    receive(&mut c, Payload::Heartbeat(Heartbeat { local_time_ms: Some(86_399_500), ..Default::default() }), 100_000);
    assert_eq!(c.local_time(100_500), NaiveTime::from_hms_opt(0, 0, 0).unwrap());
    // An absent optional value is not a request to reset the clock.
    receive(&mut c, Payload::Heartbeat(Heartbeat::default()), 101_000);
    assert_eq!(c.local_time(101_500), NaiveTime::from_hms_opt(0, 0, 1).unwrap());
    // A host timezone/manual-time change takes effect without changing input time.
    receive(
        &mut c,
        Payload::Heartbeat(Heartbeat { local_time_ms: Some(7 * 3_600_000), ..Default::default() }),
        102_000,
    );
    assert_eq!(c.local_time(103_000), NaiveTime::from_hms_opt(7, 0, 1).unwrap());
    assert!(!c.sleeping);
    // A fresh connection never treats board uptime as wall time.
    let fresh = Client::new("qa".into(), vec![2; 16], true, 9_000_000);
    assert_eq!(fresh.local_time(9_000_000), NaiveTime::from_hms_opt(0, 0, 0).unwrap());
}

#[test]
fn host_clock_rejects_out_of_range_sample() {
    let (mut c, _, _) = setup();
    let e = sync::envelope(
        &[1; 16],
        &[2; 16],
        Payload::Heartbeat(Heartbeat { local_time_ms: Some(86_400_000), ..Default::default() }),
    );
    let raw = codec::encode(&e).unwrap();
    assert!(c.receive(e, &raw, 100).is_err());
}

#[test]
fn double_tap_after_selected_session_vanishes_does_not_panic() {
    let (mut c, mut h, _) = setup();
    tap(&mut c, 60, 90, 10010);
    assert_eq!(c.scene.selected_id(), Some("claude:test"));
    // The session leaves before the host answers the optimistic selection.
    let old = h.model.clone();
    h.model.sessions.clear();
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10050).unwrap();
    }
    assert_eq!(c.intents.len(), 1);
    assert_eq!(c.scene.selected_id(), None);
    tap(&mut c, 200, 240, 10100);
    tap(&mut c, 200, 240, 10300);
    assert!(actions(&mut c).iter().all(|a| a.kind == 1));
    assert!(c.connected);
}
