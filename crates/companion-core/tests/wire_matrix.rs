use companion_core::{
    codec,
    crypto::{Cipher, Reassembly},
    sync,
    wire::{envelope::Payload, *},
};
#[test]
fn every_payload_roundtrips_through_bounded_c_decoder() {
    let messages = vec![
        Payload::Hello(Hello::default()),
        Payload::HelloAck(HelloAck::default()),
        Payload::Error(ProtocolError { code: 1, related_action_id: Some(7), detail: "test".into() }),
        Payload::Heartbeat(Heartbeat::default()),
        Payload::Resync(Resync::default()),
        Payload::SnapshotBegin(SnapshotBegin {
            revision: 1,
            view: Some(View { page: 1, ..Default::default() }),
            ..Default::default()
        }),
        Payload::Delta(Delta { base_revision: 1, revision: 2, overflow_count: Some(0), ..Default::default() }),
        Payload::Applied(Applied { revision: 1 }),
        Payload::BlobGet(BlobGet { sha256: vec![5; 32] }),
        Payload::BlobChunk(BlobChunk {
            sha256: vec![5; 32],
            total_length: 3,
            data: b"abc".to_vec(),
            r#final: true,
            ..Default::default()
        }),
        Payload::BlobUnavailable(BlobUnavailable { sha256: vec![5; 32], reason: 2 }),
        Payload::Action(Action { id: 1, kind: 4, anchor: Some(0), ..Default::default() }),
        Payload::ActionResult(ActionResult { action_id: 1, status: 3, reason: 7, ..Default::default() }),
        Payload::SnapshotPart(SnapshotPart {
            revision: 1,
            index: 0,
            sessions: vec![Session {
                id: "s".into(),
                provider: 1,
                status: 1,
                active_subagents: Some(0),
                branch: Some(String::new()),
                ..Default::default()
            }],
        }),
        Payload::SnapshotCommit(SnapshotCommit { revision: 1, parts_count: 0, sha256: vec![7; 32] }),
        Payload::BlobCancel(BlobCancel { sha256: vec![5; 32] }),
    ];
    for p in messages {
        let e = sync::envelope(&[1; 16], &[2; 16], p);
        let bytes = codec::encode(&e).unwrap();
        let encoded = codec::c_roundtrip(&bytes).unwrap();
        let decoded = codec::decode(&encoded, codec::control(e.payload.as_ref().unwrap())).unwrap();
        assert_eq!(decoded, e);
    }
}
#[test]
fn all_fragment_boundaries_including_single_byte() {
    let bytes = codec::encode(&sync::envelope(
        &[1; 16],
        &[2; 16],
        Payload::BlobChunk(BlobChunk {
            sha256: vec![1; 32],
            data: vec![2; 512],
            total_length: 512,
            r#final: true,
            ..Default::default()
        }),
    ))
    .unwrap();
    for width in 1..=473 {
        let mut tx = Cipher::new(&[3; 32], [4; 4], 1);
        let mut rx = Cipher::new(&[3; 32], [4; 4], 1);
        let mut decoder = Reassembly::default();
        let mut result = None;
        for (n, c) in bytes.chunks(width).enumerate() {
            let f = tx.seal(1, bytes.len() as u32, (n * width) as u32, c).unwrap();
            let p = rx.open(&f).unwrap();
            if let Some((_, b)) = decoder.receive(&p, n as u64).unwrap() {
                assert!(result.is_none());
                result = Some(b);
            }
        }
        assert_eq!(result.as_deref(), Some(bytes.as_slice()));
    }
}
