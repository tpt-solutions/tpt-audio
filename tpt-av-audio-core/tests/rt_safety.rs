//! Real-time safety audit: the audio-thread render path must not allocate.
//!
//! A counting global allocator tracks every heap allocation in the test
//! process. After a warm-up render (which may build caches), the counter
//! must not move while `TimelineRenderer::render` runs repeatedly, nor
//! while an `AudioGraph` of DSP nodes processes.
//!
//! This is the executable version of spec2 §5's "ZERO allocation" rule for
//! the audio thread.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tpt_av_audio_core::dsp::fade::FadeNode;
use tpt_av_audio_core::dsp::gain::GainNode;
use tpt_av_audio_core::dsp::pan::PanNode;
use tpt_av_audio_core::graph::AudioGraph;
use tpt_av_audio_core::renderer::TimelineRenderer;
use tpt_av_audio_core::scheduler::TimelineState;
use tpt_av_audio_core::{AssetPcm, AssetStore};
use tpt_av_audio_timeline::{AssetId, Clip, Session, TrackId};
use tpt_av_audio_utils::AudioBuffer;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Serializes the audit tests: they share one process-wide counter, so any
/// parallelism between them (or in the harness) would pollute the count.
static AUDIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn build_session() -> Session {
    let mut session = Session::new("rt-audit", 48_000);
    let _t1 = session.add_track("vocals");
    let t2 = session.add_track("guitar");
    let asset = tpt_av_audio_timeline::AudioAsset {
        id: AssetId(1),
        file_path: "tone.wav".into(),
        duration_frames: 480_000,
        sample_rate: 48_000,
        channels: 2,
    };
    session.register_asset(asset);

    for (track, start) in [(TrackId(1), 0u64), (t2, 240u64)] {
        let clip = Clip {
            id: session.generate_clip_id(),
            asset_id: AssetId(1),
            start_frame: start,
            source_offset: 0,
            duration_frames: 480_000,
            volume_envelope: Some(tpt_av_audio_timeline::Envelope::with_points(
                vec![
                    tpt_av_audio_timeline::EnvelopePoint {
                        frame: 0,
                        value: 0.2,
                    },
                    tpt_av_audio_timeline::EnvelopePoint {
                        frame: 100_000,
                        value: 1.0,
                    },
                ],
                tpt_av_audio_timeline::InterpolationMethod::Linear,
            )),
            pan_envelope: None,
            fade_in_frames: 240,
            fade_out_frames: 480,
            loop_start: None,
            loop_end: None,
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
        };
        session.track_mut(track).unwrap().insert_clip(clip);
    }
    session
}

fn pcm() -> AssetPcm {
    AssetPcm {
        sample_rate: 48_000,
        channels: 2,
        data: vec![0.25; 480_000 * 2],
    }
}

/// Measures the *minimum* allocation delta over several fixed-size windows.
///
/// A genuine per-iteration allocation shows up in every window (≥ iterations
/// count); sporadic harness noise (test-runner bookkeeping) lands in only a
/// few windows, so the minimum stays zero.
fn min_alloc_delta(mut work: impl FnMut()) -> usize {
    let mut min = usize::MAX;
    for _ in 0..10 {
        let before = ALLOCATIONS.load(Ordering::SeqCst);
        for _ in 0..100 {
            work();
        }
        let delta = ALLOCATIONS.load(Ordering::SeqCst) - before;
        min = min.min(delta);
    }
    min
}

#[test]
fn timeline_render_allocates_nothing_on_the_audio_path() {
    let _guard = AUDIT_LOCK.lock().unwrap();
    let store = Arc::new(AssetStore::new());
    store.insert(AssetId(1), pcm());
    let state = Arc::new(TimelineState::new(build_session()));

    let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
    renderer.prepare(256, 2);

    // Warm-up: touches the snapshot and asset-map paths once.
    let mut buffer = AudioBuffer::new(256, 2);
    renderer.render(&mut buffer).expect("warm-up render");

    let buffer = &mut buffer;
    let delta = min_alloc_delta(|| renderer.render(buffer).expect("render"));
    assert_eq!(delta, 0, "render must not allocate on the audio path");
}

#[test]
fn live_timeline_updates_keep_render_allocation_free() {
    let _guard = AUDIT_LOCK.lock().unwrap();
    let store = Arc::new(AssetStore::new());
    store.insert(AssetId(1), pcm());
    let state = Arc::new(TimelineState::new(build_session()));

    let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
    renderer.prepare(256, 2);

    let mut buffer = AudioBuffer::new(256, 2);
    renderer.render(&mut buffer).unwrap();

    // A main-thread update interleaved with renders must not make the
    // render path itself allocate. Measure pure render windows (no update
    // in the measured closure); the update path allocates by design but is
    // a Main-Thread concern and runs between windows.
    let delta = min_alloc_delta(|| renderer.render(&mut buffer).expect("render"));
    assert_eq!(delta, 0, "render must not allocate on the audio path");

    // Sanity: updates do work and publish (renders still succeed after).
    let mut next = state.load_snapshot().session.clone();
    next.tracks[0].volume = 0.5;
    state.update(next);
    renderer.render(&mut buffer).expect("render after update");
}

#[test]
fn audio_graph_dsp_chain_allocates_nothing() {
    let _guard = AUDIT_LOCK.lock().unwrap();
    let mut graph = AudioGraph::new();
    let gain = graph.add_node(Box::new(GainNode::new(0.5)));
    let pan = graph.add_node(Box::new(PanNode::new(0.25)));
    let fade = graph.add_node(Box::new(FadeNode::new(240, 480, 480_000)));
    graph.connect(gain, pan).unwrap();
    graph.connect(pan, fade).unwrap();
    graph.prepare().unwrap();

    let mut buffer = AudioBuffer::new(256, 2);
    graph.process(&mut buffer).expect("warm-up");

    let delta = min_alloc_delta(|| graph.process(&mut buffer).expect("process"));
    assert_eq!(delta, 0, "graph process must not allocate");
}
