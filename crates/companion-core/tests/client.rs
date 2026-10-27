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
