//! L2/L5: spool durability (AC0.4.11), drainer (AC0.4.12), no silent loss (AC0.4.13), sink (AC0.4.16).
mod common;
use common::spool::*;
use greprum::{
    config::{IngestConfig, SpoolConfig},
    sink::{MemorySink, Sink, SinkError, sqlx_sink},
    spool::{self, Pos, Read_, SpoolError},
};
use std::{collections::HashSet, sync::atomic::Ordering::SeqCst};

const T0: i64 = 1_760_236_800_000;

// ---- AC0.4.11 spool ---------------------------------------------------------

#[tokio::test]
async fn ac_0_4_11_a_append_then_read_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, _) = open(dir.path(), spool_cfg());
    let b = batch("a", 0, 3, T0);
    sp.append(&b).await.unwrap();
    match spool::read_at(dir.path(), Pos { seg: 1, off: 0 }).unwrap() {
        Read_::Batch(got, next) => {
            assert_eq!(*got, b);
            assert!(next.off > 8);
            assert!(matches!(
                spool::read_at(dir.path(), next).unwrap(),
                Read_::End
            ));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn ac_0_4_11_b_corrupt_frame_skipped_later_frames_survive() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, _) = open(dir.path(), spool_cfg());
    for i in 0..3 {
        sp.append(&batch("a", i, 1, T0)).await.unwrap();
    }
    // Flip a payload byte of the 2nd frame.
    let p = spool::seg_path(dir.path(), 1);
    let mut data = std::fs::read(&p).unwrap();
    let first_len = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize + 8;
    data[first_len + 8 + 5] ^= 0xff;
    std::fs::write(&p, data).unwrap();

    let mut pos = Pos { seg: 1, off: 0 };
    let (mut ok, mut corrupt) = (0, 0);
    loop {
        match spool::read_at(dir.path(), pos).unwrap() {
            Read_::Batch(_, n) => {
                ok += 1;
                pos = n
            }
            Read_::Corrupt(n) => {
                corrupt += 1;
                pos = n
            }
            Read_::End => break,
        }
    }
    assert_eq!((ok, corrupt), (2, 1));
}

#[tokio::test]
async fn ac_0_4_11_c_torn_tail_ignored_and_truncated_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let (sp, _) = open(dir.path(), spool_cfg());
        sp.append(&batch("a", 0, 2, T0)).await.unwrap();
        sp.append(&batch("a", 2, 2, T0)).await.unwrap();
    }
    let p = spool::seg_path(dir.path(), 1);
    let full = std::fs::metadata(&p).unwrap().len();
    let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
    f.set_len(full - 10).unwrap(); // tear the last frame
    drop(f);

    let first_frame_end = match spool::read_at(dir.path(), Pos { seg: 1, off: 0 }).unwrap() {
        Read_::Batch(_, n) => n,
        o => panic!("{o:?}"),
    };
    assert!(matches!(
        spool::read_at(dir.path(), first_frame_end).unwrap(),
        Read_::End
    ));

    // Reopen truncates the torn tail; a new append lands cleanly after frame one.
    let (sp, _) = open(dir.path(), spool_cfg());
    assert_eq!(std::fs::metadata(&p).unwrap().len(), first_frame_end.off);
    sp.append(&batch("a", 9, 1, T0)).await.unwrap();
    match spool::read_at(dir.path(), first_frame_end).unwrap() {
        Read_::Batch(b, _) => assert_eq!(b.events[0].seq, 9),
        o => panic!("{o:?}"),
    }
}

#[tokio::test]
async fn ac_0_4_11_d_segment_rolls_at_limit_and_reads_cross_segments() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = SpoolConfig {
        segment_bytes: 600,
        ..spool_cfg()
    };
    let (sp, _) = open(dir.path(), cfg);
    for i in 0..10 {
        sp.append(&batch("a", i, 1, T0)).await.unwrap();
    }
    assert!(spool::list_segments(dir.path()).len() >= 3);
    let mut pos = Pos { seg: 1, off: 0 };
    let mut seqs = vec![];
    while let Read_::Batch(b, n) = spool::read_at(dir.path(), pos).unwrap() {
        seqs.push(b.events[0].seq);
        pos = n;
    }
    assert_eq!(seqs, (0..10).collect::<Vec<_>>());
}

#[tokio::test]
async fn ac_0_4_11_e_concurrent_appends_all_durable() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, _) = open(
        dir.path(),
        SpoolConfig {
            strict_fsync: true,
            ..spool_cfg()
        },
    );
    let mut hs = vec![];
    for t in 0..8 {
        let sp = sp.clone();
        hs.push(tokio::spawn(async move {
            for i in 0..25 {
                sp.append(&batch(&format!("s{t}"), i, 1, T0)).await.unwrap();
            }
        }));
    }
    for h in hs {
        h.await.unwrap();
    }
    let mut pos = Pos { seg: 1, off: 0 };
    let mut n = 0;
    while let Read_::Batch(_, next) = spool::read_at(dir.path(), pos).unwrap() {
        n += 1;
        pos = next;
    }
    assert_eq!(n, 200);
}

#[tokio::test]
async fn ac_0_4_11_f_disk_cap_returns_full() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, _) = open(
        dir.path(),
        SpoolConfig {
            cap_bytes: 1500,
            ..spool_cfg()
        },
    );
    let mut full = false;
    for i in 0..50 {
        if let Err(SpoolError::Full) = sp.append(&batch("a", i, 1, T0)).await {
            full = true;
            break;
        }
    }
    assert!(full);
    assert!(sp.bytes() <= 1500);
    assert!(sp.fill_pct() > 50.0);
}

// ---- AC0.4.12 drainer -------------------------------------------------------

#[tokio::test]
async fn ac_0_4_12_a_fresh_lane_batches_at_most_500_events() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    for i in 0..24 {
        sp.append(&batch("a", i * 50, 50, T0)).await.unwrap(); // 1200 events
    }
    let sink = MemorySink::new();
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    drain_all(&mut d).await;
    assert_eq!(sink.len(), 1200);
    assert!(
        sink.max_batch.load(SeqCst) <= 500,
        "{}",
        sink.max_batch.load(SeqCst)
    );
}

#[tokio::test]
async fn ac_0_4_12_b_transient_failure_retried_then_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    sp.append(&batch("a", 0, 5, T0)).await.unwrap();
    let sink = MemorySink::new();
    sink.fail_transient.store(2, SeqCst);
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    use greprum::spool::drainer::Step;
    assert_eq!(d.step_fresh().await, Step::Retry);
    assert_eq!(d.step_fresh().await, Step::Retry);
    assert_eq!(d.step_fresh().await, Step::Wrote(5));
    assert_eq!(sink.len(), 5);
}

#[tokio::test]
async fn ac_0_4_12_c_permanent_failure_goes_to_dead_and_lane_continues() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    sp.append(&batch("good1", 0, 2, T0)).await.unwrap();
    sp.append(&batch("poison", 0, 2, T0)).await.unwrap();
    sp.append(&batch("good2", 0, 2, T0)).await.unwrap();
    let sink = MemorySink::new();
    *sink.poison_session.lock().unwrap() = Some("poison".into());
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    drain_all(&mut d).await;
    assert_eq!(sink.len(), 4);
    assert_eq!(m.dead_frames.load(SeqCst), 1);
    assert_eq!(
        std::fs::read_dir(dir.path().join("dead")).unwrap().count(),
        1
    );
}

#[tokio::test]
async fn ac_0_4_12_d_checkpoint_atomic_and_restart_resumes_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = IngestConfig {
        fresh_batch: 10,
        ..IngestConfig::default()
    };
    let sink = MemorySink::new();
    {
        let (sp, m) = open(dir.path(), spool_cfg());
        for i in 0..6 {
            sp.append(&batch("a", i * 5, 5, T0)).await.unwrap();
        }
        let mut d = drainer(&sp, &sink, &m, cfg.clone());
        d.step_fresh().await; // writes 10 events, checkpoints
        assert_eq!(sink.len(), 10);
        assert!(dir.path().join("checkpoint.json").exists());
        assert!(!dir.path().join("checkpoint.json.tmp").exists());
    } // "crash": everything dropped
    let (sp, m) = open(dir.path(), spool_cfg());
    let mut d = drainer(&sp, &sink, &m, cfg);
    drain_all(&mut d).await;
    let seqs: HashSet<_> = sink.snapshot().iter().map(|e| e.seq).collect();
    assert_eq!(sink.len(), 30, "no duplicates after resume");
    assert_eq!(seqs.len(), 30);
}

#[tokio::test]
async fn ac_0_4_12_e_stalled_fresh_lane_demoted_new_events_written_first() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    sp.append(&batch("old", 0, 3, T0)).await.unwrap();
    let sink = MemorySink::new();
    sink.outage.store(true, SeqCst);
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    use greprum::spool::drainer::Step;
    assert_eq!(d.step_fresh().await, Step::Retry);
    d.demote_fresh();
    assert_eq!(d.checkpoint().backfill.len(), 1);
    sp.append(&batch("new", 0, 2, T0 + 1000)).await.unwrap();
    sink.outage.store(false, SeqCst);
    // Fresh lane first: the new frame lands before any backfill.
    assert_eq!(d.step_fresh().await, Step::Wrote(2));
    assert_eq!(sink.snapshot()[0].session_id, "new");
    drain_all(&mut d).await;
    assert_eq!(sink.len(), 5);
    assert!(d.checkpoint().backfill.is_empty());
}

#[tokio::test]
async fn ac_0_4_12_f_finished_segments_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(
        dir.path(),
        SpoolConfig {
            segment_bytes: 600,
            ..spool_cfg()
        },
    );
    for i in 0..12 {
        sp.append(&batch("a", i, 1, T0)).await.unwrap();
    }
    let before = spool::list_segments(dir.path()).len();
    assert!(before >= 3);
    let sink = MemorySink::new();
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    drain_all(&mut d).await;
    let after = spool::list_segments(dir.path());
    assert_eq!(after.len(), 1, "only the active segment remains: {after:?}");
    assert_eq!(sink.len(), 12);
}

#[tokio::test]
async fn ac_0_4_12_g_backfill_batches_up_to_5000_events() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    let sink = MemorySink::new();
    sink.outage.store(true, SeqCst);
    for i in 0..200 {
        sp.append(&batch("a", i * 50, 50, T0)).await.unwrap(); // 10 000 events
    }
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    d.demote_fresh();
    sink.outage.store(false, SeqCst);
    drain_all(&mut d).await;
    assert_eq!(sink.len(), 10_000);
    let mb = sink.max_batch.load(SeqCst);
    assert!(mb > 500 && mb <= 5000, "{mb}");
}

// ---- AC0.4.13 no silent loss ------------------------------------------------

#[tokio::test]
async fn ac_0_4_13_a_property_accepted_equals_written_plus_dead_over_random_ops() {
    use rand::{Rng, SeedableRng, rngs::StdRng};
    for seed in 0..200u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let dir = tempfile::tempdir().unwrap();
        let cfg = IngestConfig {
            fresh_batch: 20,
            backfill_batch: 40,
            ..IngestConfig::default()
        };
        let scfg = SpoolConfig {
            segment_bytes: 2000,
            ..spool_cfg()
        };
        let sink = MemorySink::new();
        let (mut sp, mut m) = open(dir.path(), scfg.clone());
        let mut d = drainer(&sp, &sink, &m, cfg.clone());
        let mut accepted: Vec<(String, i32)> = vec![];
        let mut next_seq = 0;
        for _ in 0..40 {
            match rng.gen_range(0..6) {
                0..=2 => {
                    let n = rng.gen_range(1..6);
                    sp.append(&batch("p", next_seq, n, T0)).await.unwrap();
                    accepted.extend((next_seq..next_seq + n).map(|s| ("p".to_string(), s)));
                    next_seq += n;
                }
                3 => {
                    d.step_fresh().await;
                }
                4 => {
                    sink.outage.store(rng.gen_bool(0.5), SeqCst);
                    if sink.outage.load(SeqCst) {
                        d.step_fresh().await;
                        if rng.gen_bool(0.5) {
                            d.demote_fresh();
                        }
                    }
                }
                _ => {
                    // crash + restart (no step is in flight at this point)
                    drop(d);
                    drop(sp);
                    (sp, m) = open(dir.path(), scfg.clone());
                    d = drainer(&sp, &sink, &m, cfg.clone());
                }
            }
        }
        sink.outage.store(false, SeqCst);
        drain_all(&mut d).await;
        let mut got: Vec<(String, i32)> = sink
            .snapshot()
            .into_iter()
            .map(|e| (e.session_id, e.seq))
            .collect();
        got.sort();
        let mut want = accepted.clone();
        want.sort();
        assert_eq!(got, want, "seed {seed}");
    }
}

#[tokio::test]
async fn ac_0_4_13_b_crash_mid_drain_replays_everything_exactly_once() {
    let dir = tempfile::tempdir().unwrap();
    let sink = MemorySink::new();
    let cfg = IngestConfig {
        fresh_batch: 7,
        ..IngestConfig::default()
    };
    {
        let (sp, m) = open(dir.path(), spool_cfg());
        for i in 0..10 {
            sp.append(&batch("a", i * 3, 3, T0)).await.unwrap();
        }
        let mut d = drainer(&sp, &sink, &m, cfg.clone());
        d.step_fresh().await;
    }
    let (sp, m) = open(dir.path(), spool_cfg());
    let mut d = drainer(&sp, &sink, &m, cfg);
    drain_all(&mut d).await;
    let mut seqs: Vec<i32> = sink.snapshot().iter().map(|e| e.seq).collect();
    seqs.sort();
    assert_eq!(seqs, (0..30).collect::<Vec<_>>());
}

#[tokio::test]
async fn ac_0_4_13_c_ten_minute_outage_then_recovery_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    let sink = MemorySink::new();
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    sink.outage.store(true, SeqCst);
    let mut total = 0;
    // Virtual 10 minutes at one batch per second.
    for sec in 0..600 {
        sp.append(&batch("a", sec * 2, 2, T0 + sec as i64 * 1000))
            .await
            .unwrap();
        total += 2;
        d.step_fresh().await;
        if sec % 30 == 29 {
            d.demote_fresh(); // fresh_stall_secs elapsed
        }
    }
    assert_eq!(sink.len(), 0);
    assert!(d.oldest_unwritten_age_ms(T0 + 600_000) >= 599_000);
    sink.outage.store(false, SeqCst);
    drain_all(&mut d).await;
    assert_eq!(sink.len(), total);
    assert_eq!(d.oldest_unwritten_age_ms(T0 + 700_000), 0);
}

#[tokio::test]
async fn ac_0_4_13_d_steady_state_oldest_unwritten_age_under_five_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    let sink = MemorySink::new();
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    let mut worst = 0;
    for i in 0..50 {
        let now = T0 + i * 1000;
        sp.append(&batch("a", i as i32, 1, now)).await.unwrap();
        worst = worst.max(d.oldest_unwritten_age_ms(now + 100)); // sampled right after append
        d.step_fresh().await;
    }
    assert!(worst < 5000, "{worst}");
    assert_eq!(sink.len(), 50);
}

// ---- AC0.4.16 sink ----------------------------------------------------------

#[tokio::test]
async fn ac_0_4_16_a_memory_sink_contract() {
    let s = MemorySink::new();
    s.write(&[event("a", 0), event("a", 1)]).await.unwrap();
    assert_eq!(s.len(), 2);
    s.fail_transient.store(1, SeqCst);
    assert!(matches!(
        s.write(&[event("a", 2)]).await,
        Err(SinkError::Transient(_))
    ));
    assert_eq!(s.len(), 2);
}

#[test]
fn ac_0_4_16_b_insert_chunking_stays_under_placeholder_limit() {
    assert!(sqlx_sink::MAX_ROWS_PER_INSERT * sqlx_sink::INSERT_COLUMNS.len() < 65_535);
    let rows: Vec<_> = (0..3).map(|i| event("a", i)).collect();
    let qb = sqlx_sink::build_insert(&rows);
    let sql = qb.sql();
    assert_eq!(
        sql.matches('?').count(),
        3 * sqlx_sink::INSERT_COLUMNS.len()
    );
    assert!(sql.contains("`path`") && sql.contains("`release`"));
    assert!(!sql.contains("/"), "values are bound, never inlined");
}

#[test]
fn ac_0_4_16_c_column_list_matches_stored_event_fields() {
    let v = serde_json::to_value(event("a", 0)).unwrap();
    let fields: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    let cols: Vec<String> = sqlx_sink::INSERT_COLUMNS
        .iter()
        .map(|c| c.trim_matches(|ch| ch == '"' || ch == '`').to_string())
        .collect();
    let mut a = fields.clone();
    let mut b = cols.clone();
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[tokio::test]
async fn ac_0_4_12_h_unreachable_database_is_a_transient_error_never_fatal() {
    // Port 1 refuses connections. The gateway must still construct its sink and keep spooling.
    let sink =
        sqlx_sink::SqlxSink::lazy("mysql://root@127.0.0.1:1/public").expect("lazy never connects");
    let r = sink.write(&[event("a", 0)]).await;
    assert!(matches!(r, Err(SinkError::Transient(_))), "{r:?}");
}

#[tokio::test]
async fn ac_0_4_12_i_lagging_fresh_lane_hands_its_backlog_to_the_backfill_lane() {
    let dir = tempfile::tempdir().unwrap();
    let (sp, m) = open(dir.path(), spool_cfg());
    // 3 000 events received 10 s ago: further behind than fresh_lag_secs (5 s).
    for i in 0..60 {
        sp.append(&batch("a", i * 50, 50, T0)).await.unwrap();
    }
    let sink = MemorySink::new();
    let mut d = drainer(&sp, &sink, &m, IngestConfig::default());
    assert!(!d.maybe_demote_lagging(T0 + 4_000), "within the lag budget");
    assert!(d.maybe_demote_lagging(T0 + 10_000), "10 s behind");
    assert_eq!(d.checkpoint().backfill.len(), 1);
    // New events are written by the fresh lane first, then the backlog in big inserts.
    sp.append(&batch("new", 0, 5, T0 + 10_000)).await.unwrap();
    use greprum::spool::drainer::Step;
    assert_eq!(d.step_fresh().await, Step::Wrote(5));
    assert_eq!(sink.snapshot()[0].session_id, "new");
    drain_all(&mut d).await;
    assert_eq!(sink.len(), 3005);
    assert!(
        sink.max_batch.load(SeqCst) > 500,
        "backlog drained with larger inserts"
    );
}
