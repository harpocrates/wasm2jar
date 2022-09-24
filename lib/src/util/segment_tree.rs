use std::borrow::Borrow;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::ops::RangeInclusive;

/// Simplified segment tree
///
/// Intervals get cloned once per inner node on which they get stored. Note however, that you can
/// wrap any interval in an `Rc` or a reference and still have an interval (but one that can be
/// cheaply cloned).
#[derive(Debug)]
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
            if let SegmentNode::Inner {
                left_child,
                right_child,
                ..
            } = node
            {
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
            1 => SegmentNode::Leaf {
                endpoint: endpoints[0],
                intervals: vec![],
            },
            n => {
                let (left_endpoints, right_endpoints) = endpoints.split_at(n / 2);
                let from = endpoints[0];
                let until = endpoints[n - 1];
                let left_child = Box::new(Self::build_empty(left_endpoints));
                let right_child = Box::new(Self::build_empty(right_endpoints));
                SegmentNode::Inner {
                    from,
                    until,
                    left_child,
                    right_child,
                    intervals: vec![],
                }
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
            if let SegmentNode::Inner {
                ref mut left_child,
                ref mut right_child,
                ..
            } = node
            {
                to_visit.push(left_child);
                to_visit.push(right_child);
            }
        }
    }
}

/// Internal node in the segment tree
#[derive(Debug)]
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
    type Endpoint: Ord + Copy + std::fmt::Debug;

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

impl<Idx: Copy + Ord + std::fmt::Debug> Interval for RangeInclusive<Idx> {
    type Endpoint = Idx;

    fn from(&self) -> Idx {
        *self.start()
    }

    fn until(&self) -> Idx {
        *self.end()
    }
}


#[cfg(test)]
mod test {
    use super::*;
    use std::ops::RangeInclusive;
    use std::collections::HashSet;
    use std::hash::Hash;

    fn intervals_set<I: Hash + Interval + Clone + Eq + PartialEq>(tree: &SegmentTree<I>, point: I::Endpoint) -> HashSet<I> {
        tree.intervals_containing(&point).into_iter().cloned().collect()
    }

    #[test]
    fn no_intervals() {
        let tree: SegmentTree<RangeInclusive<i32>> = SegmentTree::new(vec![]);
        assert!(intervals_set(&tree, 0).is_empty());
        assert!(intervals_set(&tree, 1).is_empty());
        assert!(intervals_set(&tree, 2).is_empty());
    }

    #[test]
    fn single_interval() {
        let tree: SegmentTree<RangeInclusive<i32>> = SegmentTree::new(vec![1..=3]);
        assert!(intervals_set(&tree, 0).is_empty());
        assert_eq!(intervals_set(&tree, 1), HashSet::from([1..=3]));
        assert_eq!(intervals_set(&tree, 2), HashSet::from([1..=3]));
        assert_eq!(intervals_set(&tree, 3), HashSet::from([1..=3]));
        assert!(intervals_set(&tree, 4).is_empty());
    }

    #[test]
    fn two_overlapping_intervals() {
        let tree: SegmentTree<RangeInclusive<i32>> = SegmentTree::new(vec![1..=3, 2..=4]);
        assert!(intervals_set(&tree, 0).is_empty());
        assert_eq!(intervals_set(&tree, 1), HashSet::from([1..=3]));
        assert_eq!(intervals_set(&tree, 2), HashSet::from([1..=3, 2..=4]));
        assert_eq!(intervals_set(&tree, 3), HashSet::from([1..=3, 2..=4]));
        assert_eq!(intervals_set(&tree, 4), HashSet::from([2..=4]));
        assert!(intervals_set(&tree, 5).is_empty());
    }


    //cargo test util::segment_tree::test::multiple_overlapping_intervals -- --show-output
    #[test]
    fn multiple_overlapping_intervals() {
        let tree: SegmentTree<RangeInclusive<i32>> = SegmentTree::new(vec![0..=2, 2..=4, 4..=6, 2..=8, 0..=10]);
        println!("{:#?}", tree);
        assert!(intervals_set(&tree, -1).is_empty());
     //   assert_eq!(intervals_set(&tree, 0), HashSet::from([0..=2, 0..=10]));
        assert_eq!(intervals_set(&tree, 1), HashSet::from([0..=2, 0..=10]));
        assert_eq!(intervals_set(&tree, 2), HashSet::from([0..=2, 0..=10, 2..=4, 2..=8]));
        assert_eq!(intervals_set(&tree, 3), HashSet::from([0..=10, 2..=4, 2..=8]));
        assert_eq!(intervals_set(&tree, 4), HashSet::from([0..=10, 2..=4, 2..=8, 4..=6]));
        assert_eq!(intervals_set(&tree, 5), HashSet::from([0..=10, 2..=8, 4..=6]));
        assert_eq!(intervals_set(&tree, 6), HashSet::from([0..=10, 2..=8, 4..=6]));
        assert_eq!(intervals_set(&tree, 7), HashSet::from([0..=10, 2..=8]));
        assert_eq!(intervals_set(&tree, 8), HashSet::from([0..=10, 2..=8]));
        assert_eq!(intervals_set(&tree, 9), HashSet::from([0..=10]));
        assert_eq!(intervals_set(&tree, 10), HashSet::from([0..=10]));
        assert!(intervals_set(&tree, 11).is_empty());
    }
}
