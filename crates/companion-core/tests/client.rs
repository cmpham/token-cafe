use companion_core::{
    authority::{Authority, Peer},
    client::Client,
    codec,
    model::Model,
    sync,
    wire::{envelope::Payload, *},
};
use std::collections::BTreeMap;

#[test]
fn desktop_hint_covers_startup_and_disconnect_until_snapshot() {
    let mut expected = vec![0; 480 * 272];
    companion_core::client::render_desktop_hint(&mut expected);
    let mut frame = vec![0; 480 * 272];
    let mut waiting = Client::new("qa".into(), vec![1; 16], true, 0);
    waiting.render(&mut frame, 0);
    assert_eq!(frame, expected);

    let (mut synced, _, _) = setup();
    synced.render(&mut frame, 10_000);
    assert_ne!(frame, expected);
    synced.sleeping = true;
    synced.disconnect();
    assert!(!synced.sleeping, "connection loss must wake the instruction screen");
    synced.render(&mut frame, 10_001);
    assert_eq!(frame, expected);
    assert!(!synced.armed(10_001));
    tap(&mut synced, 250, 240, 10_002);
    assert!(actions(&mut synced).is_empty());
}

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

#[test]
fn delayed_desktop_completion_does_not_replay_over_new_review() {
    let (mut c, mut host, mut peer) = setup();
    tap(&mut c, 60, 90, 10010);
    tap(&mut c, 60, 90, 10200);
    let aa = actions(&mut c);
    for a in &aa {
        let (result, _) = host.action(&mut peer, a, 10230, None).unwrap();
        receive(&mut c, Payload::ActionResult(result), 10240);
    }
    for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10250).unwrap();
    }
    // OS navigation is still pending when the user opens details again.
    tap(&mut c, 200, 240, 10500);
    tap(&mut c, 200, 240, 10700);
    let a = actions(&mut c).pop().unwrap();
    assert_eq!(a.kind, 2);
    let body = b"printf 'synthetic approval fixture'\nNo actual command is executed.";
    let (result, _) = host.action(&mut peer, &a, 10740, Some(body)).unwrap();
    receive(&mut c, Payload::ActionResult(result), 10750);
    for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10760).unwrap();
    }
    assert_eq!(c.view().page, 2, "an older pending handoff must not close newer details");
    assert!(c.scene.review.is_some());
}

#[test]
fn switching_patrons_keeps_only_selected_text_without_stale_details() {
    let (mut c, mut host, _) = setup();
    let other_body = b"A different Codex task";
    let blob = sync::blob(other_body).unwrap();
    host.model.sessions.insert(
        "codex:other".into(),
        Session {
            id: "codex:other".into(),
            title: "Other".into(),
            provider: 2,
            status: 1,
            seat: 2,
            task_blob: Some(blob),
            ..Default::default()
        },
    );
    host.model.revision += 1;
    for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10001).unwrap();
    }
    for b in sync::chunks(other_body).unwrap() {
        c.blobs.receive(&b).unwrap();
    }
    for t in (10100..=20000).step_by(100) {
        c.tick(t).unwrap();
        while c.outgoing().is_some() {}
    }
    for (x, id, text, t) in [
        (60, "claude:test", "synthetic approval fixture", 20100),
        (300, "codex:other", "A different Codex task", 20700),
        (60, "claude:test", "synthetic approval fixture", 21300),
    ] {
        tap(&mut c, x, 90, t);
        assert_eq!(c.scene.details().len(), 1);
        assert_eq!(c.scene.details()[0].id, id);
        assert!(c.scene.details()[0].task.contains(text));
    }
}

fn publish_attention_snapshot(c: &mut Client, host: &mut Authority, now: u64) {
    host.model.revision += 1;
    for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, now).unwrap();
    }
}

#[test]
fn new_attention_wakes_for_a_minute_but_refresh_does_not() {
    let (mut c, mut host, _) = setup();
    c.tick(610_000).unwrap();
    assert!(c.sleeping);
    publish_attention_snapshot(&mut c, &mut host, 610_010);
    assert!(c.sleeping, "same waiting request must not wake repeatedly");
    let mut question = host.model.sessions["claude:test"].clone();
    question.id = "codex:question".into();
    question.provider = 2;
    question.seat = 2;
    question.waiting_reason = 2;
    question.approval = None;
    host.model.sessions.insert(question.id.clone(), question);
    publish_attention_snapshot(&mut c, &mut host, 611_000);
    assert!(!c.sleeping);
    assert!(!c.armed(611_001));
    c.tick(670_999).unwrap();
    assert!(!c.sleeping);
    publish_attention_snapshot(&mut c, &mut host, 670_999);
    c.tick(671_000).unwrap();
    assert!(c.sleeping, "refresh cannot extend attention wake time");
    // A different request identity on the same waiting patron is a new alert.
    let r = host.model.sessions.get_mut("claude:test").unwrap().approval.as_mut().unwrap();
    r.id = "qa-2".into();
    let b = r.body_blob.as_ref().unwrap();
    r.digest = companion_core::crypto::request_digest(
        &r.id,
        "claude:test",
        1,
        &r.title,
        &r.tool_name,
        &b.sha256.clone().try_into().unwrap(),
        b.byte_length,
        &[1, 2],
    )
    .unwrap()
    .to_vec();
    publish_attention_snapshot(&mut c, &mut host, 672_000);
    assert!(!c.sleeping);
    assert!(actions(&mut c).iter().all(|a| a.kind != 7));
}

#[test]
fn attention_footer_opens_review_without_a_decision() {
    let (mut c, _, _) = setup();
    assert!(c.scene.selected_id().is_none());
    tap(&mut c, 300, 260, 10_900);
    assert!(c.scene.review.is_none(), "status text must not open review");
    assert!(actions(&mut c).iter().all(|a| a.kind == 1));
    tap(&mut c, 100, 252, 11_000);
    let sent = actions(&mut c);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, 2);
    assert_eq!(sent[0].session_id, "claude:test");
    assert!(c.scene.review.as_ref().is_some_and(|r| r.decision.is_none()));
    assert!(!c.armed(11_031), "new review must be presented before it can arm");
}

#[test]
fn attention_next_is_selection_only_and_new_request_cancels_held_contact() {
    let (mut c, mut host, _) = setup();
    c.touch(1, 100, 260, 11_000, &mut |_| true).unwrap();
    let mut question = host.model.sessions["claude:test"].clone();
    question.id = "codex:question".into();
    question.provider = 2;
    question.seat = 2;
    question.waiting_reason = 2;
    question.approval = None;
    host.model.sessions.insert(question.id.clone(), question);
    publish_attention_snapshot(&mut c, &mut host, 11_010);
    c.touch(0, 100, 260, 11_030, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty(), "new request cancels a held View gesture");
    tap(&mut c, 450, 240, 12_000);
    let sent = actions(&mut c);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, 1);
    assert_eq!(sent[0].session_id, "codex:question");
    assert_eq!(c.scene.attention_id(), Some("codex:question"));
    assert!(c.scene.review.is_none());
}

#[test]
fn creature_rosters_reach_board_through_snapshot_and_delta() {
    let (mut client, mut host, _) = setup();
    for (index, roster) in ["pokemon", "dragons", "mixed", "humans"].iter().enumerate() {
        let previous = host.model.clone();
        host.model.revision += 1;
        host.model.view.patron_ids = cafe_ui::patrons::Patrons::roster(roster).unwrap().ids().to_vec();
        let packets = if index == 0 {
            sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap()
        } else {
            sync::update(&previous, &host.model, &[1; 16], &[2; 16]).unwrap()
        };
        for e in packets {
            let raw = codec::encode(&e).unwrap();
            client.receive(e, &raw, 10_000).unwrap();
        }
        assert_eq!(client.scene.patrons.ids().as_slice(), host.model.view.patron_ids);
    }
    for ids in [vec![1; 7], vec![1; 9], vec![17; 8]] {
        host.model.view.patron_ids = ids;
        assert!(companion_core::model::validate(&host.model).is_err());
    }
}

fn arm_external_button(c: &mut Client) {
    open(c);
    let mut frame = vec![0; 480 * 272];
    c.render(&mut frame, 10350);
    c.presented(10350);
    c.tick(11000).unwrap();
    assert!(c.armed(11000));
    actions(c);
}
#[test]
fn physical_button_requires_review_and_fresh_press_then_persists_once() {
    let (mut c, _, _) = setup();
    assert!(c.approval_waiting());
    c.approval_button(true, 10001, &mut |_| true).unwrap();
    c.tick(10020).unwrap(); // Normal loop sync must not discard a navigation click.
    c.approval_button(false, 10050, &mut |_| panic!("opening cannot approve")).unwrap();
    let opened = actions(&mut c);
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].kind, 2);
    assert_eq!(opened[0].request_id, "qa-1");
    assert_eq!(c.view().page, 2);
    let mut frame = vec![0; 480 * 272];
    c.render(&mut frame, 10350);
    c.presented(10350);
    c.tick(11000).unwrap();
    assert!(c.armed(11000));
    c.approval_button(false, 11000, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty(), "release alone cannot approve");
    c.approval_button(true, 11010, &mut |_| true).unwrap();
    c.approval_button(true, 11020, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty(), "press does not dispatch before release");
    c.tick(11030).unwrap();
    c.approval_button(false, 11060, &mut |_| true).unwrap();
    let aa = actions(&mut c);
    assert_eq!(aa.len(), 1);
    assert_eq!((aa[0].kind, aa[0].decision), (7, 1));
    assert_eq!(aa[0].request_id, "qa-1");
    assert_eq!(c.receipts.len(), 1);
    assert!(!c.approval_waiting());
    c.approval_button(true, 12000, &mut |_| true).unwrap();
    c.approval_button(false, 12050, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty());
}
#[test]
fn physical_button_rejects_withdrawal_resolution_and_disconnect_during_press() {
    for mode in 0..4 {
        let (mut c, mut h, _) = setup();
        arm_external_button(&mut c);
        c.approval_button(true, 11010, &mut |_| true).unwrap();
        if mode == 0 {
            c.disconnect();
        } else {
            let old = h.model.clone();
            let s = h.model.sessions.get_mut("claude:test").unwrap();
            match mode {
                1 => s.approval = None,
                2 => s.approval.as_mut().unwrap().state = 6,
                _ => s.approval.as_mut().unwrap().state = 5,
            }
            h.model.revision += 1;
            for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
                let raw = codec::encode(&e).unwrap();
                c.receive(e, &raw, 11030).unwrap();
            }
        }
        c.approval_button(false, 11060, &mut |_| panic!("stale press must not persist")).unwrap();
        assert!(actions(&mut c).iter().all(|a| a.kind != 7));
        assert_eq!(c.approval_waiting(), mode == 1, "unlinked waiting still needs attention");
    }
}
#[test]
fn physical_button_cancels_for_touch_long_hold_sleep_and_storage_failure() {
    for mode in 0..4 {
        let (mut c, _, _) = setup();
        arm_external_button(&mut c);
        if mode == 2 {
            c.sleeping = true;
        }
        c.approval_button(true, 11010, &mut |_| true).unwrap();
        if mode == 0 {
            c.touch(2, 0, 0, 11020, &mut |_| true).unwrap();
        }
        let release = if mode == 1 { 12000 } else { 11060 };
        c.approval_button(false, release, &mut |_| mode != 3).unwrap();
        assert!(actions(&mut c).iter().all(|a| a.kind != 7));
        assert!(c.receipts.is_empty());
    }
}
#[test]
fn physical_button_held_before_review_never_approves_after_it_opens() {
    let (mut c, _, _) = setup();
    c.approval_button(true, 10001, &mut |_| true).unwrap();
    arm_external_button(&mut c);
    c.approval_button(false, 11060, &mut |_| panic!("held before review")).unwrap();
    assert!(actions(&mut c).is_empty());
}
#[test]
fn physical_button_survives_unrelated_model_update_without_switching_request() {
    let (mut c, mut h, _) = setup();
    arm_external_button(&mut c);
    c.approval_button(true, 11010, &mut |_| true).unwrap();
    let old = h.model.clone();
    h.model.revision += 1;
    h.model.usage.tokens_display = "123".into();
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 11030).unwrap();
    }
    c.approval_button(false, 11060, &mut |_| true).unwrap();
    assert_eq!(actions(&mut c).iter().filter(|a| a.kind == 7).count(), 1);
}

#[test]
fn physical_button_does_not_bypass_lcd_presentation_or_desktop_only_review() {
    for present in [false, true] {
        let (mut c, _, _) = setup();
        open(&mut c);
        if present {
            let mut f = vec![0; 480 * 272];
            c.render(&mut f, 10350);
            c.presented(10350);
            c.scene.review.as_mut().unwrap().allow_approve = false;
        }
        actions(&mut c);
        c.tick(11000).unwrap();
        c.approval_button(true, 11010, &mut |_| panic!("not eligible")).unwrap();
        c.approval_button(false, 11060, &mut |_| panic!("not eligible")).unwrap();
        assert!(actions(&mut c).is_empty());
    }
}

#[test]
fn physical_button_cannot_skip_loading_or_switch_steps_while_held() {
    let (mut c, _, _) = setup();
    c.approval_button(true, 10010, &mut |_| true).unwrap();
    c.approval_button(false, 10050, &mut |_| true).unwrap();
    actions(&mut c);
    c.approval_button(true, 10060, &mut |_| true).unwrap();
    let mut frame = vec![0; 480 * 272];
    c.render(&mut frame, 10070);
    c.presented(10070);
    c.tick(10600).unwrap();
    c.approval_button(false, 10650, &mut |_| panic!("loading click cannot approve")).unwrap();
    assert!(actions(&mut c).is_empty());
    assert!(c.receipts.is_empty());
}

#[test]
fn physical_button_leaves_completed_review_without_approving_again() {
    let (mut c, mut h, _) = setup();
    arm_external_button(&mut c);
    let old = h.model.clone();
    h.model.sessions.get_mut("claude:test").unwrap().approval.as_mut().unwrap().state = 6;
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 11010).unwrap();
    }
    c.approval_button(true, 11100, &mut |_| true).unwrap();
    c.tick(11120).unwrap();
    c.approval_button(false, 11150, &mut |_| panic!("back cannot approve")).unwrap();
    let aa = actions(&mut c);
    assert_eq!(aa.len(), 1);
    assert_eq!(aa[0].kind, 3);
    assert_ne!(c.view().page, 2);
}

#[test]
fn physical_button_acknowledges_desktop_open_before_separate_approval() {
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
    c.approval_button(true, 12000, &mut |_| panic!("ack is not approval")).unwrap();
    c.approval_button(false, 12030, &mut |_| panic!("ack is not approval")).unwrap();
    c.render(&mut f, 12100);
    c.presented(12100);
    c.tick(13000).unwrap();
    assert!(c.armed(13000));
    c.approval_button(true, 13010, &mut |_| true).unwrap();
    c.tick(13030).unwrap();
    c.approval_button(false, 13060, &mut |_| true).unwrap();
    let aa = actions(&mut c);
    assert_eq!(aa.len(), 1);
    assert_eq!((aa[0].kind, aa[0].decision), (7, 1));
}

#[test]
fn physical_button_open_is_cancelled_when_waiting_request_changes() {
    let (mut c, mut h, _) = setup();
    c.approval_button(true, 10010, &mut |_| true).unwrap();
    let old = h.model.clone();
    let r = h.model.sessions.get_mut("claude:test").unwrap().approval.as_mut().unwrap();
    r.id = "replacement".into();
    let b = r.body_blob.as_ref().unwrap();
    r.digest = companion_core::crypto::request_digest(
        &r.id,
        "claude:test",
        1,
        &r.title,
        &r.tool_name,
        &b.sha256.clone().try_into().unwrap(),
        b.byte_length,
        &[1, 2],
    )
    .unwrap()
    .to_vec();
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10030).unwrap();
    }
    c.approval_button(false, 10060, &mut |_| panic!("replaced request")).unwrap();
    assert!(actions(&mut c).is_empty());
    assert_ne!(c.view().page, 2);
}

#[test]
fn attention_button_opens_desktop_for_unlinked_questions_and_permissions() {
    for why in [1, 2] {
        for review_open in [false, true] {
            let (mut c, mut h, _) = setup();
            if review_open {
                arm_external_button(&mut c);
            }
            let old = h.model.clone();
            let s = h.model.sessions.get_mut("claude:test").unwrap();
            s.approval = None;
            s.waiting_reason = why;
            s.unlinked_review_blob = s.task_blob.clone();
            if review_open {
                h.model.view = c.view();
                h.model.view.review_kind = 3;
                h.model.view.review_request_id.clear();
            }
            h.model.revision += 1;
            for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
                let raw = codec::encode(&e).unwrap();
                c.receive(e, &raw, 12000).unwrap();
            }
            actions(&mut c);
            assert!(c.approval_waiting());
            c.approval_button(true, 12100, &mut |_| panic!("desktop handoff cannot decide")).unwrap();
            c.tick(12120).unwrap();
            c.approval_button(false, 12160, &mut |_| panic!("desktop handoff cannot decide")).unwrap();
            let aa = actions(&mut c);
            assert_eq!(aa.len(), 1);
            assert_eq!((aa[0].kind, aa[0].session_id.as_str()), (6, "claude:test"));
            assert!(c.receipts.is_empty());
            assert!(!c.approval_waiting(), "opening the app acknowledges the current alert");
            c.tick(12180).unwrap();
            assert!(!c.approval_waiting(), "refresh must not rearm an acknowledged item");
            let old = h.model.clone();
            h.model.sessions.get_mut("claude:test").unwrap().status = 2;
            h.model.revision += 1;
            for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
                let raw = codec::encode(&e).unwrap();
                c.receive(e, &raw, 12200).unwrap();
            }
            assert!(!c.approval_waiting());
        }
    }
}

#[test]
fn attention_button_routes_desktop_only_and_choice_requests_to_desktop() {
    for choices in [false, true] {
        let (mut c, mut h, _) = setup();
        let old = h.model.clone();
        let session = h.model.sessions.get_mut("claude:test").unwrap();
        if choices {
            session.provider = 2;
            session.seat = 2;
        }
        let r = session.approval.as_mut().unwrap();
        if choices {
            r.allowed_decisions = vec![3, 4];
        } else {
            r.desktop_only_reason = "CONTINUE ON DESKTOP".into();
            r.allowed_decisions.clear();
        }
        let b = r.body_blob.as_ref().unwrap();
        r.digest = companion_core::crypto::request_digest(
            &r.id,
            "claude:test",
            if choices { 2 } else { 1 },
            &r.title,
            &r.tool_name,
            &b.sha256.clone().try_into().unwrap(),
            b.byte_length,
            &r.allowed_decisions.iter().map(|d| *d as u8).collect::<Vec<_>>(),
        )
        .unwrap()
        .to_vec();
        h.model.revision += 1;
        for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
            let raw = codec::encode(&e).unwrap();
            c.receive(e, &raw, 12000).unwrap();
        }
        actions(&mut c);
        assert!(c.approval_waiting());
        c.approval_button(true, 12100, &mut |_| panic!("no approval")).unwrap();
        c.approval_button(false, 12160, &mut |_| panic!("no approval")).unwrap();
        let aa = actions(&mut c);
        assert_eq!(aa.len(), 1);
        assert_eq!(aa[0].kind, 6);
        assert!(c.receipts.is_empty());
    }
}

#[test]
fn attention_desktop_click_cancels_if_item_resolves_during_press() {
    let (mut c, mut h, _) = setup();
    let old = h.model.clone();
    let s = h.model.sessions.get_mut("claude:test").unwrap();
    s.approval = None;
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 12000).unwrap();
    }
    actions(&mut c);
    c.approval_button(true, 12100, &mut |_| true).unwrap();
    let old = h.model.clone();
    h.model.sessions.get_mut("claude:test").unwrap().status = 2;
    h.model.revision += 1;
    for e in sync::update(&old, &h.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 12130).unwrap();
    }
    c.approval_button(false, 12160, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty());
    assert!(!c.approval_waiting());
}

#[test]
fn attention_led_acknowledges_screen_selection_and_rearms_for_new_request() {
    let (mut c, mut h, _) = setup();
    assert!(c.approval_waiting());
    tap(&mut c, 60, 90, 10010);
    assert!(!c.approval_waiting(), "screen selection acknowledges attention");
    actions(&mut c);
    publish_attention_snapshot(&mut c, &mut h, 11000);
    assert!(!c.approval_waiting(), "same snapshot cannot renew the alert");
    let r = h.model.sessions.get_mut("claude:test").unwrap().approval.as_mut().unwrap();
    r.id = "new-question".into();
    let b = r.body_blob.as_ref().unwrap();
    r.digest = companion_core::crypto::request_digest(
        &r.id,
        "claude:test",
        1,
        &r.title,
        &r.tool_name,
        &b.sha256.clone().try_into().unwrap(),
        b.byte_length,
        &[1, 2],
    )
    .unwrap()
    .to_vec();
    publish_attention_snapshot(&mut c, &mut h, 12000);
    assert!(c.approval_waiting(), "new request on the same session needs a fresh acknowledgement");
    c.approval_button(true, 12100, &mut |_| panic!("open only")).unwrap();
    c.approval_button(false, 12160, &mut |_| panic!("open only")).unwrap();
    assert!(!c.approval_waiting(), "a completed physical click acknowledges attention");
    assert!(c.receipts.is_empty(), "acknowledgement never implies approval");
}

#[test]
fn attention_led_ignores_cancelled_and_long_button_presses() {
    for cancelled in [false, true] {
        let (mut c, _, _) = setup();
        c.approval_button(true, 10010, &mut |_| true).unwrap();
        if cancelled {
            c.touch(2, 0, 0, 10030, &mut |_| true).unwrap();
        }
        c.approval_button(false, 11000, &mut |_| true).unwrap();
        assert!(c.approval_waiting(), "only completed interactions acknowledge attention");
        assert!(actions(&mut c).is_empty());
    }
}

#[test]
fn attention_led_acknowledges_current_batch_without_marking_requests_resolved() {
    let (mut c, mut h, _) = setup();
    let mut other = h.model.sessions["claude:test"].clone();
    other.id = "claude:other".into();
    other.seat = 1;
    other.approval = None;
    h.model.sessions.insert(other.id.clone(), other);
    publish_attention_snapshot(&mut c, &mut h, 12000);
    assert!(c.approval_waiting());
    c.approval_button(true, 12100, &mut |_| panic!("attention is not approval")).unwrap();
    c.approval_button(false, 12160, &mut |_| panic!("attention is not approval")).unwrap();
    assert!(!c.approval_waiting(), "one interaction silences the current alert batch");
    assert!(c.replica.model.sessions.values().all(|s| s.status == 3));
    assert!(c.receipts.is_empty());
    publish_attention_snapshot(&mut c, &mut h, 12200);
    assert!(!c.approval_waiting());
}
#[test]
fn keypad_navigation_and_app_shortcuts_never_answer_requests() {
    let (mut c, mut host, mut peer) = setup();
    c.keyboard(11, 1000).unwrap();
    c.keyboard(4, 1010).unwrap();
    let a = actions(&mut c);
    assert_eq!(a.iter().map(|a| a.kind).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(c.view().page, 2);
    let before = c.view();
    for command in [7, 8] {
        c.keyboard(command, 2000).unwrap();
        let a = actions(&mut c);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].kind, i32::from(command) + 1);
        assert_eq!(c.view(), before, "app switch must preserve the review");
        let model = host.model.clone();
        let (r, d) = host.action(&mut peer, &a[0], 2000 + u64::from(command) * 1000, None).unwrap();
        assert_eq!(r.status, 2);
        assert_eq!(d, Some(companion_core::authority::Dispatch::App { codex: command == 8 }));
        assert_eq!(host.model, model);
        assert!(host.action(&mut peer, &a[0], 99999, None).unwrap().1.is_none(), "retry cannot launch twice");
        host.finish(&mut peer, &a[0], true);
    }
    c.keyboard(9, 3000).unwrap();
    let handoff = actions(&mut c);
    assert_eq!(
        handoff.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [6],
        "knob press in review opens desktop, never decides"
    );
    assert_eq!(handoff[0].session_id, before.selected_session_id);
    c.keyboard(11, 4000).unwrap();
    assert_eq!(c.view().page, 1);
    assert!(actions(&mut c).iter().all(|a| a.kind != 7));
    c.disconnect();
    for command in 1..=11 {
        c.keyboard(command, 5000).unwrap();
    }
    assert!(actions(&mut c).is_empty(), "no offline commands are queued");
}

#[test]
fn keypad_app_commands_require_navigation_permission() {
    let (_, mut host, _) = setup();
    let mut peer = Peer::new("unprivileged".into(), vec![1; 16], false, false);
    for kind in [8, 9, 11, 12] {
        let action = Action { id: kind as u64, kind, ..Default::default() };
        let (r, dispatch) = host.action(&mut peer, &action, 10000, None).unwrap();
        assert_eq!(r.status, 3);
        assert!(dispatch.is_none());
    }
}

#[test]
fn knob_press_from_details_hands_off_the_exact_session_once() {
    for (kind, navigable) in [(2, true), (3, true), (3, false)] {
        let (mut c, mut host, mut peer) = setup();
        let s = host.model.sessions.get_mut("claude:test").unwrap();
        s.approval = None;
        s.navigation_available = navigable;
        if kind == 3 {
            s.unlinked_review_blob = s.task_blob.clone();
        }
        host.model.revision += 1;
        for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
            let raw = codec::encode(&e).unwrap();
            c.receive(e, &raw, 10010).unwrap();
        }
        actions(&mut c);
        c.keyboard(11, 11000).unwrap();
        c.keyboard(9, 11100).unwrap();
        assert_eq!(c.view().page, 2);
        assert_eq!(c.view().review_kind, kind);
        for a in actions(&mut c) {
            assert!(matches!(a.kind, 1 | 2));
            assert!(host.action(&mut peer, &a, 11110, None).unwrap().1.is_none());
        }
        c.keyboard(9, 11200).unwrap();
        let handoff = actions(&mut c);
        if navigable {
            assert_eq!(handoff.len(), 1);
            assert_eq!(handoff[0].kind, 6);
            assert_eq!(handoff[0].session_id, "claude:test");
            let (_, dispatch) = host.action(&mut peer, &handoff[0], 11210, None).unwrap();
            assert_eq!(dispatch, Some(companion_core::authority::Dispatch::Desktop("claude:test".into())));
            assert!(host.action(&mut peer, &handoff[0], 11220, None).unwrap().1.is_none());
        } else {
            assert!(handoff.is_empty());
            assert_eq!(c.view().page, 2);
        }
        assert!(c.receipts.is_empty());
    }
}

#[test]
fn knob_always_browses_and_top_keys_scroll_without_changing_agents() {
    let (mut c, mut host, _) = setup();
    let body = (0..80).map(|i| format!("Task detail line {i:02}\n")).collect::<String>();
    let blob = sync::blob(body.as_bytes()).unwrap();
    let first = host.model.sessions.get_mut("claude:test").unwrap();
    first.approval = None;
    first.task_blob = Some(blob);
    let mut second = first.clone();
    second.id = "codex:other".into();
    second.provider = 2;
    second.seat = 2;
    host.model.sessions.insert(second.id.clone(), second);
    host.model.revision += 1;
    for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
        let raw = codec::encode(&e).unwrap();
        c.receive(e, &raw, 10010).unwrap();
    }
    for chunk in sync::chunks(body.as_bytes()).unwrap() {
        c.blobs.receive(&chunk).unwrap();
    }
    actions(&mut c);

    for (time, id) in [(11000, "claude:test"), (11100, "codex:other")] {
        c.keyboard(11, time).unwrap();
        assert_eq!(c.view().selected_session_id, id);
        assert_eq!(c.view().page, 1, "turning only highlights the next agent");
        assert!(c.scene.review.is_none());
        assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [1]);
    }
    c.keyboard(9, 11200).unwrap();
    assert_eq!(c.view().page, 2);
    assert_eq!(c.view().selected_session_id, "codex:other");
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [2]);

    c.keyboard(2, 11300).unwrap();
    assert_eq!(c.view().selected_session_id, "codex:other", "scrolling cannot switch agents");
    assert!(c.view().review_anchor > 0);
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [5]);
    c.keyboard(1, 11400).unwrap();
    assert_eq!(c.view().review_anchor, 0);
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [5]);

    c.keyboard(11, 11600).unwrap();
    assert_eq!(c.view().selected_session_id, "claude:test");
    assert_eq!(c.view().page, 1, "turning browses even after opening a review");
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [1]);
    c.keyboard(2, 11700).unwrap();
    assert!(c.view().task_anchor > 0);
    assert_eq!(c.view().selected_session_id, "claude:test");
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [4]);
    c.keyboard(1, 11800).unwrap();
    assert_eq!(c.view().task_anchor, 0);
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [4]);
}

#[test]
fn every_keypad_control_wakes_screen_and_restarts_idle_timer() {
    for connected in [true, false] {
        for command in 1..=11 {
            let (mut c, _, _) = setup();
            if !connected {
                c.disconnect();
            }
            actions(&mut c);
            c.tick(600_000).unwrap();
            assert!(c.sleeping, "fixture must reach actual idle sleep");

            let pressed_at = 600_100;
            c.keyboard(command, pressed_at).unwrap();
            assert!(!c.sleeping, "command {command}, connected={connected} must wake immediately");
            if !connected {
                assert!(actions(&mut c).is_empty(), "offline wake must not queue shortcuts");
            }
            c.tick(pressed_at + 33).unwrap();
            assert!(!c.sleeping, "the next frame must remain awake");
            c.tick(pressed_at + 599_999).unwrap();
            assert!(!c.sleeping, "every control restarts the full idle interval");
            c.tick(pressed_at + 600_000).unwrap();
            assert!(c.sleeping, "ordinary idle sleep still applies after keypad wake");
        }
    }
}

#[test]
fn keypad_cancels_a_held_external_approval_and_wakes_without_queuing() {
    let (mut c, _, _) = setup();
    arm_external_button(&mut c);
    c.approval_button(true, 12000, &mut |_| true).unwrap();
    c.keyboard(7, 12020).unwrap();
    c.approval_button(false, 12050, &mut |_| true).unwrap();
    assert!(actions(&mut c).iter().all(|a| a.kind != 7));
    c.disconnect();
    c.sleeping = true;
    c.keyboard(8, 13000).unwrap();
    assert!(!c.sleeping);
    assert!(actions(&mut c).is_empty());
}

#[test]
fn approval_view_updates_cannot_switch_the_knob_from_browsing_to_scrolling() {
    let (mut c, mut host, mut peer) = setup();
    let body = (0..80).map(|i| format!("Approval detail {i:02}\n")).collect::<String>();
    let blob = sync::blob(body.as_bytes()).unwrap();
    let first = host.model.sessions.get_mut("claude:test").unwrap();
    let r = first.approval.as_mut().unwrap();
    r.body_blob = Some(blob.clone());
    r.digest = companion_core::crypto::request_digest(
        &r.id,
        &first.id,
        first.provider as u8,
        &r.title,
        &r.tool_name,
        &blob.sha256.as_slice().try_into().unwrap(),
        blob.byte_length,
        &[1, 2],
    )
    .unwrap()
    .to_vec();
    let mut second = first.clone();
    second.id = "codex:other".into();
    second.provider = 2;
    second.seat = 2;
    second.approval = None;
    host.model.sessions.insert(second.id.clone(), second);
    host.model.revision += 1;
    let publish = |c: &mut Client, host: &Authority, now| {
        for e in sync::snapshot(&host.model, &[1; 16], &[2; 16]).unwrap() {
            let raw = codec::encode(&e).unwrap();
            c.receive(e, &raw, now).unwrap();
        }
    };
    publish(&mut c, &host, 10010);
    for chunk in sync::chunks(body.as_bytes()).unwrap() {
        c.blobs.receive(&chunk).unwrap();
    }
    actions(&mut c);
    c.keyboard(11, 11000).unwrap();
    assert_eq!(c.view().selected_session_id, "claude:test");
    assert_eq!(c.view().page, 1);
    let select = actions(&mut c);
    assert_eq!(select.len(), 1);
    assert_eq!(select[0].kind, 1);
    let (result, _) = host.action(&mut peer, &select[0], 11010, None).unwrap();
    receive(&mut c, Payload::ActionResult(result), 11020);
    publish(&mut c, &host, 11030);

    // A desktop/synchronized approval can appear between physical turns.
    let r = host.model.sessions["claude:test"].approval.as_ref().unwrap();
    host.model.view.page = 2;
    host.model.view.review_kind = 1;
    host.model.view.review_request_id = r.id.clone();
    host.model.view.review_blob = r.body_blob.clone();
    host.model.revision += 1;
    publish(&mut c, &host, 11100);
    assert_eq!(c.view().page, 2);
    c.keyboard(11, 11200).unwrap();
    assert_eq!(c.view().selected_session_id, "codex:other", "approval visibility cannot capture knob turns");
    assert_eq!(c.view().page, 1);
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [1]);

    c.keyboard(10, 11300).unwrap();
    assert_eq!(c.view().selected_session_id, "claude:test");
    assert_eq!(c.view().page, 1);
    actions(&mut c);
    c.keyboard(9, 11400).unwrap();
    assert_eq!(c.view().page, 2);
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [2]);
    c.keyboard(2, 11500).unwrap();
    assert_eq!(c.view().selected_session_id, "claude:test");
    assert!(c.view().review_anchor > 0, "only the top keys scroll the approval");
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [5]);
    c.keyboard(11, 11700).unwrap();
    assert_eq!(c.view().selected_session_id, "codex:other");
    assert_eq!(c.view().page, 1);
    assert_eq!(actions(&mut c).iter().map(|a| a.kind).collect::<Vec<_>>(), [1]);
}

#[test]
fn finishing_long_keypad_setup_starts_a_fresh_idle_timer() {
    let mut c = Client::new("qa".into(), vec![1; 16], true, 0);
    // Calibration can take longer than ten minutes; uptime is not idle time.
    let completed_at = 2_700_000;
    c.sleeping = true;
    c.input_activity(completed_at - 33);
    c.tick(completed_at).unwrap();
    assert!(!c.sleeping);
    c.input_activity(completed_at); // overlay closes after final input
    c.tick(completed_at + 33).unwrap();
    assert!(!c.sleeping, "finishing setup must leave the cafe visible");
    c.tick(completed_at + 599_999).unwrap();
    assert!(!c.sleeping);
    c.tick(completed_at + 600_000).unwrap();
    assert!(c.sleeping, "ordinary idle sleep still works after setup");
}

#[test]
fn keypad_setup_cancels_hidden_approval_gestures() {
    let (mut c, _, _) = setup();
    arm_external_button(&mut c);
    c.approval_button(true, 12000, &mut |_| true).unwrap();
    c.input_activity(12020);
    c.approval_button(false, 12050, &mut |_| true).unwrap();
    assert!(actions(&mut c).is_empty());
    assert!(c.receipts.is_empty());
}

#[test]
fn oled_keeps_knob_browse_hint_in_details_and_clears_private_state_offline() {
    let (mut c, _, _) = setup();
    c.keyboard(11, 11000).unwrap();
    let rows = c.keyboard_display();
    assert!(rows[0].starts_with(b"BROWSE"));
    assert!(rows[1].starts_with(b"TEST CLAUDE"));
    assert!(rows[2].starts_with(b"NEEDS INPUT"));
    assert!(rows[3].starts_with(b"TURN>AGT PUSH>RD"));
    c.keyboard(9, 11200).unwrap();
    assert!(c.keyboard_display()[0].starts_with(b"READ"));
    assert!(c.keyboard_display()[3].starts_with(b"TURN>AGT PUSH>PC"));
    c.keyboard(11, 11400).unwrap();
    assert!(c.keyboard_display()[0].starts_with(b"BROWSE"));
    c.disconnect();
    let rows = c.keyboard_display();
    assert!(rows[0].starts_with(b"CAFE OFFLINE"));
    assert!(rows[1].starts_with(b"START TOKEN CAFE"));
    assert!(rows.iter().flatten().all(|b| (32..=126).contains(b)));
}

#[test]
fn middle_select_and_knob_are_identical_on_cafe_and_details() {
    let (mut key, _, _) = setup();
    let (mut knob, _, _) = setup();
    for client in [&mut key, &mut knob] {
        client.keyboard(11, 11000).unwrap();
        actions(client);
    }
    for time in [11100, 11200] {
        key.keyboard(4, time).unwrap();
        knob.keyboard(9, time).unwrap();
        assert_eq!(key.view(), knob.view());
        assert_eq!(actions(&mut key), actions(&mut knob));
    }
}

#[test]
fn voice_and_toggle_preserve_review_and_voice_targets_selected_session_once() {
    use companion_core::authority::Dispatch;
    for (command, kind) in [(5, 11), (6, 12)] {
        let (mut c, mut host, mut peer) = setup();
        c.keyboard(11, 11000).unwrap();
        c.keyboard(4, 11100).unwrap();
        for a in actions(&mut c) {
            host.action(&mut peer, &a, 11110, None).unwrap();
        }
        let before = c.view();
        let model = host.model.clone();
        c.keyboard(command, 11200).unwrap();
        assert_eq!(c.view(), before);
        let issued = actions(&mut c);
        assert_eq!(issued.len(), 1);
        assert_eq!(issued[0].kind, kind);
        let (r, dispatch) = host.action(&mut peer, &issued[0], 11210, None).unwrap();
        assert_eq!(r.status, 2);
        assert_eq!(
            dispatch,
            Some(if kind == 12 { Dispatch::Voice("claude:test".into()) } else { Dispatch::ToggleApp })
        );
        assert_eq!(host.model, model);
        assert!(host.action(&mut peer, &issued[0], 11220, None).unwrap().1.is_none());
        let failed = host.finish(&mut peer, &issued[0], false);
        assert_eq!(failed.status, 3);
        assert_eq!(host.model, model, "native voice never answers the approval");
    }
}

#[test]
fn voice_requires_a_live_desktop_target() {
    let (mut c, mut host, mut peer) = setup();
    c.keyboard(6, 11000).unwrap();
    assert!(actions(&mut c).is_empty());
    host.model.sessions.get_mut("claude:test").unwrap().navigation_available = false;
    for id in ["claude:test", "claude:missing"] {
        let a = Action {
            id: if id == "claude:test" { 1 } else { 2 },
            kind: 12,
            session_id: id.into(),
            ..Default::default()
        };
        let (r, dispatch) = host.action(&mut peer, &a, 12000, None).unwrap();
        assert_eq!(r.status, 3);
        assert!(dispatch.is_none());
    }
}

#[test]
fn patron_key_cycles_every_theme_without_changing_selection_or_requests() {
    use cafe_ui::patrons::Patrons;
    let (mut c, mut host, mut peer) = setup();
    c.keyboard(11, 11000).unwrap();
    c.keyboard(4, 11100).unwrap();
    for a in actions(&mut c) {
        host.action(&mut peer, &a, 11110, None).unwrap();
    }
    let sessions = host.model.sessions.clone();
    let review = host.model.view.review_blob.clone();
    for (i, name) in ["dragons", "pokemon", "mixed", "humans"].iter().enumerate() {
        c.keyboard(3, 12000 + i as u64 * 1000).unwrap();
        let issued = actions(&mut c);
        assert_eq!(issued.len(), 1);
        let (r, dispatch) = host.action(&mut peer, &issued[0], 12010 + i as u64 * 1000, None).unwrap();
        assert_eq!(r.status, 1);
        assert!(dispatch.is_none());
        assert_eq!(host.model.view.patron_ids, Patrons::roster(name).unwrap().ids());
        let view = host.model.view.clone();
        host.action(&mut peer, &issued[0], 13000, None).unwrap();
        assert_eq!(host.model.view, view, "a retry cannot cycle twice");
        assert_eq!(host.model.view.selected_session_id, "claude:test");
        assert_eq!(host.model.view.review_blob, review);
        assert_eq!(host.model.sessions, sessions);
    }
}

#[test]
fn voice_feedback_reports_failure_without_claiming_recording_started() {
    let (mut c, mut host, mut peer) = setup();
    c.keyboard(11, 11000).unwrap();
    for a in actions(&mut c) {
        host.action(&mut peer, &a, 11010, None).unwrap();
    }
    c.keyboard(6, 11100).unwrap();
    assert!(c.keyboard_display()[3].starts_with(b"OPENING VOICE"));
    let issued = actions(&mut c);
    host.action(&mut peer, &issued[0], 11110, None).unwrap();
    let failed = host.finish(&mut peer, &issued[0], false);
    receive(&mut c, Payload::ActionResult(failed), 11200);
    assert!(c.keyboard_display()[3].starts_with(b"CHECK VOICE APP"));
    c.disconnect();
    assert!(c.keyboard_display()[0].starts_with(b"CAFE OFFLINE"));
}

#[test]
fn mac_keypad_activity_wakes_board_once_without_replaying_input() {
    let (mut c, _, _) = setup();
    receive(&mut c, Payload::Heartbeat(Heartbeat { keypad_activity: Some(10), ..Default::default() }), 1);
    c.tick(600_000).unwrap();
    assert!(c.sleeping);
    receive(&mut c, Payload::Heartbeat(Heartbeat { keypad_activity: Some(11), ..Default::default() }), 600_100);
    assert!(!c.sleeping);
    assert!(actions(&mut c).is_empty());
    c.tick(1_200_099).unwrap();
    assert!(!c.sleeping);
    receive(&mut c, Payload::Heartbeat(Heartbeat { keypad_activity: Some(11), ..Default::default() }), 1_200_099);
    c.tick(1_200_100).unwrap();
    assert!(c.sleeping, "a repeated heartbeat must not extend the idle timer");
}
