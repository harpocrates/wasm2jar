use std::rc::Rc;
use std::borrow::Borrow;
use std::collections::BTreeSet;

/// Simplified segment tree
///
/// Intervals get cloned once per inner node on which they get stored. Note however, that you can
/// wrap any interval in an `Rc` or a reference and still have an interval (but one that can be
/// cheaply cloned).
pub struct SegmentTree<I: Interval + Clone>(Option<SegmentNode<I>>);

impl<I: Interval + Clone> SegmentTree<I> {

    /// Find all intervals containing the specified point
    pub fn intervals_containing(&self, point: &I::Endpoint) -> Vec<&I> {
        let mut node = if let Some(root_node) = self.0.as_ref() {
            root_node
        } else {
            return vec![];
        };

        if !node.contains(point) {
            return vec![];
        }

        // These are all the intervals encountered while searching for the point
        let mut containing_intervals: Vec<&I> = vec![];
        loop {

            // All intervals on this node contain the point
            for interval in node.intervals() {
                containing_intervals.push(interval.borrow());
            }

            // Select at most one of the children (if there are any) to continue the search
            if let SegmentNode::Inner { left_child, right_child, .. } = node {
                if left_child.contains(point) {
                    node = left_child;
                    continue;
                } else if right_child.contains(point) {
                    node = right_child;
                    continue;
                }
            }
            break;
        }

        containing_intervals
    }

    /// Make a new segment tree containing all the specified intervals
    pub fn new(intervals: Vec<I>) -> SegmentTree<I> {

        // Collect and sort all endpoints
        let endpoints: &[I::Endpoint] = &intervals
            .iter()
            .flat_map(|interval| [interval.from(), interval.until()])
            .collect::<BTreeSet<I::Endpoint>>()
            .into_iter()
            .collect::<Vec<I::Endpoint>>();

        if endpoints.is_empty() {
            return SegmentTree(None);
        }

        // Build up an empty segment tree based on elementary intervals
        let mut tree = SegmentTree(Some(Self::build_empty(endpoints)));

        // Add in all of the intervals
        for interval in intervals {
            tree.insert(interval);
        }

        tree
    }

    /// Build a new empty segment tree with the specified endpoints as elementary intervals
    fn build_empty(endpoints: &[I::Endpoint]) -> SegmentNode<I> {
        match endpoints.len() {
            0 => unreachable!(),
            1 => SegmentNode::Leaf { endpoint: endpoints[0], intervals: vec![] },
            n => {
                let (left_endpoints, right_endpoints) = endpoints.split_at(n / 2);
                let from = endpoints[0];
                let until = endpoints[n - 1];
                let left_child = Box::new(Self::build_empty(left_endpoints));
                let right_child = Box::new(Self::build_empty(right_endpoints));
                SegmentNode::Inner { from, until, left_child, right_child, intervals: vec![] }
            }
        }
    }

    /// Insert a new interval in the tree
    ///
    /// Invariant: the interval endpoints should already be elementary intervals!
    fn insert(&mut self, interval: I) {
        let mut to_visit: Vec<&mut SegmentNode<I>> = vec![];
        if let Some(root) = self.0.as_mut() {
            to_visit.push(root);
        }

        while let Some(node) = to_visit.pop() {
            // If the node's interval is in the input interval, add the interval to the node
            if node.contained_in(&interval) {
                node.push_interval(interval.clone());
                continue;
            }

            // Otherwise, repeat with the left and right children
            if let SegmentNode::Inner { ref mut left_child, ref mut right_child, .. } = node {
                to_visit.push(left_child);
                to_visit.push(right_child);
            }
        }

    }

}

/// Internal node in the segment tree
enum SegmentNode<I: Interval> {
    Leaf {
        /// Single endpoint contained in the segment
        endpoint: I::Endpoint,

        /// Intervals contained at the node
        intervals: Vec<I>,
    },
    Inner {
        /// Start (inclusive) of the segment
        from: I::Endpoint,

        /// End (inclusive) of the segment
        until: I::Endpoint,

        /// Left child
        left_child: Box<SegmentNode<I>>,

        /// Right child
        right_child: Box<SegmentNode<I>>,

        /// Intervals contained at the node
        intervals: Vec<I>,
    },
}

impl<I: Interval + Clone> SegmentNode<I> {

    /// Is a point represented by this node?
    fn contains(&self, point: &I::Endpoint) -> bool {
        match self {
            SegmentNode::Leaf { endpoint, .. } => point == endpoint,
            SegmentNode::Inner { from, until, .. } => from <= point && point <= until,
        }
    }

    /// All of the intervals on this node
    fn intervals(&self) -> &[I] {
        match self {
            SegmentNode::Leaf { intervals, .. } => intervals,
            SegmentNode::Inner { intervals, .. } => intervals,
        }
    }

    /// Add an interval to this node
    fn push_interval(&mut self, interval: I) {
        match self {
            SegmentNode::Leaf { intervals, .. } => intervals.push(interval),
            SegmentNode::Inner { intervals, .. } => intervals.push(interval),
        }
    }

    /// Is the interval of this node fully contained inside the other interval?
    fn contained_in(&self, other: &I) -> bool {
        other.from() <= self.from() && self.until() <= other.until()
    }
}

/// Closed interval
pub trait Interval {

    type Endpoint: Ord + Copy;

    /// Start of the interval (inclusive)
    fn from(&self) -> Self::Endpoint;

    /// End of the interval (inclusive)
    fn until(&self) -> Self::Endpoint;
}

impl<I: Interval> Interval for SegmentNode<I> {

    type Endpoint = I::Endpoint;

    fn from(&self) -> Self::Endpoint {
        match self {
            SegmentNode::Leaf { endpoint, .. } => *endpoint,
            SegmentNode::Inner { from, .. } => *from,
        }
    }

    fn until(&self) -> Self::Endpoint {
        match self {
            SegmentNode::Leaf { endpoint, .. } => *endpoint,
            SegmentNode::Inner { until, .. } => *until,
        }
    }
}

impl<I: Interval> Interval for &I {

    type Endpoint = I::Endpoint;

    fn from(&self) -> Self::Endpoint {
        Interval::from(*self)
    }

    fn until(&self) -> Self::Endpoint {
        Interval::until(*self)
    }
}

impl<I: Interval> Interval for Rc<I> {

    type Endpoint = I::Endpoint;

    fn from(&self) -> Self::Endpoint {
        let interval: &I = self.borrow();
        interval.from()
    }

    fn until(&self) -> Self::Endpoint {
        let interval: &I = self.borrow();
        interval.until()
    }
}


