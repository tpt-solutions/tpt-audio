//! The audio processing graph: nodes connected into a processing order.
//!
//! Semantics (documented contract, spec2 §4.2): the graph processes a single
//! shared interleaved bus. Nodes run in topological edge order and either
//! transform the bus in place (insert effects) or, for generator nodes
//! ([`AudioNode::input_channels`] == 0), their output is mixed into the bus.
//! Channel counts must match the bus. This models chains (`gain → pan →
//! fade`) and parallel generators (summed); multi-bus routing is future work
//! tracked in `tpt-av-audio-plugin` (buses/side-chains).
//!
//! Real-time safety: the node list and processing order are fixed before the
//! graph runs ([`AudioGraph::prepare`]); `process` itself performs no
//! allocation, locking, or panicking.

use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// A single processing node in the audio graph.
///
/// # Real-Time Safety
///
/// [`AudioNode::process`] MUST be allocation-free, lock-free, and
/// panic-free. It runs on the audio thread.
pub trait AudioNode: Send {
    /// Processes one buffer of audio.
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError>;

    /// Number of input channels consumed (0 for generator nodes).
    fn input_channels(&self) -> u16;

    /// Number of output channels produced.
    fn output_channels(&self) -> u16;

    /// Node name for diagnostics.
    fn name(&self) -> &str {
        "node"
    }
}

/// Identifier of a node inside an [`AudioGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

/// A directed connection between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioEdge {
    pub from: NodeId,
    pub to: NodeId,
}

/// The main audio processing graph (summing-bus semantics; see module docs).
#[derive(Default)]
pub struct AudioGraph {
    nodes: Vec<Box<dyn AudioNode>>,
    edges: Vec<AudioEdge>,
    order: Vec<usize>,
    prepared: bool,
}

impl AudioGraph {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node; returns its id. Adding nodes after [`Self::prepare`]
    /// requires calling `prepare` again (Main Thread only).
    pub fn add_node(&mut self, node: Box<dyn AudioNode>) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(node);
        self.prepared = false;
        id
    }

    /// Connects `from → to`. Rejects connections that would create a cycle.
    pub fn connect(&mut self, from: NodeId, to: NodeId) -> Result<(), AudioError> {
        let exists =
            |a: NodeId, b: NodeId| self.nodes.get(a.0).is_some() && self.nodes.get(b.0).is_some();
        if !exists(from, to) {
            return Err(AudioError::InvalidConfig(format!(
                "cannot connect node {} → {} (unknown node)",
                from.0, to.0
            )));
        }
        if self.has_path(to, from) {
            return Err(AudioError::InvalidConfig(format!(
                "edge {} → {} would create a cycle",
                from.0, to.0
            )));
        }
        self.edges.push(AudioEdge { from, to });
        self.prepared = false;
        Ok(())
    }

    /// Whether a directed path exists `from → … → to` via DFS.
    fn has_path(&self, from: NodeId, to: NodeId) -> bool {
        let mut stack = vec![from];
        let mut seen = vec![false; self.nodes.len()];
        while let Some(n) = stack.pop() {
            if n == to {
                return true;
            }
            if n.0 >= seen.len() || seen[n.0] {
                continue;
            }
            seen[n.0] = true;
            for e in &self.edges {
                if e.from == n {
                    stack.push(e.to);
                }
            }
        }
        false
    }

    /// Computes the processing order (Kahn topological sort). Call on the
    /// Main Thread after building the graph.
    pub fn prepare(&mut self) -> Result<(), AudioError> {
        let n = self.nodes.len();
        let mut indegree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for e in &self.edges {
            indegree[e.to.0] += 1;
            adj[e.from.0].push(e.to.0);
        }

        let mut queue: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
        let mut order = Vec::with_capacity(n);
        while let Some(i) = queue.pop() {
            order.push(i);
            for &j in &adj[i] {
                indegree[j] -= 1;
                if indegree[j] == 0 {
                    queue.push(j);
                }
            }
        }

        if order.len() != n {
            return Err(AudioError::InvalidConfig(
                "graph contains a cycle; topological sort failed".into(),
            ));
        }
        self.order = order;
        self.prepared = true;
        Ok(())
    }

    /// Whether [`Self::prepare`] has been called since the last mutation.
    pub fn is_prepared(&self) -> bool {
        self.prepared
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Processes the bus through every node in topological order.
    ///
    /// # Real-Time Safety
    ///
    /// No allocation, no locking, no panics. All slicing is bounds-checked
    /// into `Result`s.
    pub fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        if !self.prepared {
            return Err(AudioError::InvalidConfig(
                "graph not prepared; call prepare() on the main thread".into(),
            ));
        }

        let channels = buffer.channels;
        for &idx in &self.order {
            let node = &mut self.nodes[idx];
            if node.output_channels() != channels {
                return Err(AudioError::InvalidConfig(format!(
                    "node {} has {} output channels, bus has {channels}",
                    node.name(),
                    node.output_channels()
                )));
            }
            // Generator nodes (input_channels == 0) render additively into
            // the bus; effect nodes transform it in place. Both come through
            // the same `process` call — the node's contract decides.
            if node.input_channels() != 0 && node.input_channels() != channels {
                return Err(AudioError::InvalidConfig(format!(
                    "node {} expects {} input channels, bus has {channels}",
                    node.name(),
                    node.input_channels()
                )));
            }
            node.process(buffer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::gain::GainNode;

    #[test]
    fn processes_nodes_in_order() {
        // Two gains applied in sequence: 0.5 then 0.5 → 0.25 total.
        let mut graph = AudioGraph::new();
        let a = graph.add_node(Box::new(GainNode::new(0.5)));
        let b = graph.add_node(Box::new(GainNode::new(0.5)));
        graph.connect(a, b).unwrap();
        graph.prepare().unwrap();

        let mut buf = AudioBuffer::new(1, 2);
        buf.write_frame(0, &[1.0, 1.0]).unwrap();
        graph.process(&mut buf).unwrap();

        let mut out = [0.0f32; 2];
        buf.read_frame(0, &mut out).unwrap();
        assert!((out[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn connect_rejects_cycles() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(Box::new(GainNode::new(1.0)));
        let b = graph.add_node(Box::new(GainNode::new(1.0)));
        graph.connect(a, b).unwrap();
        assert!(graph.connect(b, a).is_err());
    }

    #[test]
    fn connect_rejects_unknown_nodes() {
        let mut graph = AudioGraph::new();
        let a = graph.add_node(Box::new(GainNode::new(1.0)));
        assert!(graph.connect(NodeId(99), a).is_err());
    }

    #[test]
    fn prepare_rejects_unsorted_cycle_graph() {
        // Build a cycle without connect()'s guard to test prepare's guard.
        let mut graph = AudioGraph::new();
        let a = graph.add_node(Box::new(GainNode::new(1.0)));
        let b = graph.add_node(Box::new(GainNode::new(1.0)));
        graph.edges.push(AudioEdge { from: a, to: b });
        graph.edges.push(AudioEdge { from: b, to: a });
        assert!(graph.prepare().is_err());
    }

    #[test]
    fn process_requires_prepare() {
        let mut graph = AudioGraph::new();
        graph.add_node(Box::new(GainNode::new(1.0)));
        let mut buf = AudioBuffer::new(1, 2);
        assert!(graph.process(&mut buf).is_err());
    }

    #[test]
    fn process_rejects_channel_mismatch() {
        struct MonoNode;
        impl AudioNode for MonoNode {
            fn process(&mut self, _buffer: &mut AudioBuffer) -> Result<(), AudioError> {
                Ok(())
            }
            fn input_channels(&self) -> u16 {
                1
            }
            fn output_channels(&self) -> u16 {
                1
            }
        }
        let mut graph = AudioGraph::new();
        graph.add_node(Box::new(MonoNode));
        graph.prepare().unwrap();
        let mut buf = AudioBuffer::new(1, 2);
        assert!(graph.process(&mut buf).is_err());
    }
}
