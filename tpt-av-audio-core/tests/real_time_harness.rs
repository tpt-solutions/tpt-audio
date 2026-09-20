//! Real-time safety gate wired to the shared `tpt-av-test` harness.
//!
//! The `rt_safety.rs` audit in this package uses a hand-rolled counting
//! allocator with windowed minimums; this test is the same guarantee
//! expressed through `tpt-av-test-benchmark`, so every TPT repository's
//! real-time assertions come from one implementation. The harness's
//! `TrackingAllocator` is installed explicitly for this test binary via the
//! crate's `tracking-allocator` API (the package opts out of the default
//! feature because `rt_safety.rs` declares its own allocator).

use tpt_av_audio_core::dsp::fade::FadeNode;
use tpt_av_audio_core::dsp::gain::GainNode;
use tpt_av_audio_core::dsp::pan::PanNode;
use tpt_av_audio_core::graph::AudioGraph;
use tpt_av_audio_utils::AudioBuffer;
use tpt_av_test_benchmark::allocation_tracker::TrackingAllocator;

#[global_allocator]
static RT_ALLOCATOR: TrackingAllocator = TrackingAllocator;

/// A gain → pan → fade chain, the shape of a typical insert chain.
fn dsp_chain() -> AudioGraph {
    let mut graph = AudioGraph::new();
    let gain = graph.add_node(Box::new(GainNode::new(0.5)));
    let pan = graph.add_node(Box::new(PanNode::new(0.25)));
    let fade = graph.add_node(Box::new(FadeNode::new(240, 480, 480_000)));
    graph.connect(gain, pan).expect("connect gain -> pan");
    graph.connect(pan, fade).expect("connect pan -> fade");
    graph.prepare().expect("prepare graph");
    graph
}

#[test]
fn dsp_graph_process_is_real_time_safe_per_512_frame_callback() {
    let mut graph = dsp_chain();
    let mut buffer = AudioBuffer::new(512, 2);

    // Warm-up outside the tracked scope (caches, lazy state).
    graph.process(&mut buffer).expect("warm-up render");

    // The gate: 64 simulated callbacks with not one heap allocation.
    tpt_av_test_benchmark::assert_real_time_safe!("AudioGraph::process @512", {
        for _ in 0..64 {
            graph.process(&mut buffer).expect("render");
        }
    });
}

#[test]
fn dsp_graph_callbacks_stay_within_a_generous_wall_clock_ceiling() {
    let mut graph = dsp_chain();
    let mut buffer = AudioBuffer::new(512, 2);
    graph.process(&mut buffer).expect("warm-up render");

    // Wall-clock on a shared CI host is noisy, so the ceiling is 100x the
    // 512-frame budget (~10.67 ms at 48 kHz); the deterministic gate above
    // is allocation counting.
    let ceiling = tpt_av_test_benchmark::audio_block::block_duration(512, 48_000) * 100;
    let (_result, duration) = tpt_av_test_benchmark::timing::measure(|| {
        for _ in 0..64 {
            graph.process(&mut buffer).expect("render");
        }
    });
    assert!(
        duration <= ceiling,
        "64 callbacks took {duration:?}, ceiling {ceiling:?}"
    );
}
