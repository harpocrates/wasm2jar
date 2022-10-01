use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::{BTreeSet, VecDeque};
use std::ops::{Bound, RangeBounds};

/// Simplified segment tree
///
/// Intervals in the segment tree get cloned once per inner node on which they get stored. Consider
/// wrapping them in `Rc` or a reference if the `Clone` needs to be made cheaper.
#[derive(Debug)]
pub struct SegmentTree<Endpoint, Interval>(Box<[SegmentNode<Endpoint, Interval>]>);

type BoundInterval<E> = (Bound<E>, Bound<E>);

impl<Endpoint: Ord + Copy, Interval: Clone + RangeBounds<Endpoint>>
    SegmentTree<Endpoint, Interval>
{
    /// Find all intervals containing the specified point
    pub fn intervals_containing(&self, point: &Endpoint) -> Vec<&Interval> {
        let mut node_idx = 0;

        // These are all the intervals encountered while searching for the point
        let mut containing_intervals: Vec<&Interval> = vec![];
        loop {
            let node = &self.0[node_idx];

            // All intervals on this node contain the point
            for interval in node.intervals() {
                containing_intervals.push(interval.borrow());
            }

            // If the node isn't a leaf, select the child to continue the search
            if let SegmentNode::Inner {
                mid,
                mid_is_on_left,
                ..
            } = node
            {
                let go_left = match point.cmp(mid) {
                    Ordering::Greater => false,
                    Ordering::Equal => *mid_is_on_left,
                    Ordering::Less => true,
                };

                // Recursive into the left or right child
                node_idx = 2 * node_idx + if go_left { 1 } else { 2 };
            } else {
                break;
            }
        }

        containing_intervals
    }

    /// Make a new segment tree containing all the specified intervals
    pub fn new(intervals: Vec<Interval>) -> Self {
        // Collect and sort all endpoints
        let endpoints: &[Endpoint] = &intervals
            .iter()
            .flat_map(|interval| [interval.start_bound(), interval.end_bound()])
            .filter_map(|bound| match bound {
                Bound::Unbounded => None,
                Bound::Included(e) => Some(*e),
                Bound::Excluded(e) => Some(*e),
            })
            .collect::<BTreeSet<Endpoint>>()
            .into_iter()
            .collect::<Vec<Endpoint>>();

        // Build the tree from those endpoints
        let nodes = Self::build_tree(endpoints);
        let mut tree = SegmentTree(nodes);

        // Add in all of the intervals
        for interval in intervals {
            tree.insert(interval);
        }

        for node in tree.0.iter_mut() {
            node.intervals_mut().shrink_to_fit();
        }

        tree
    }

    /// Build the segment tree from a distinct, sorted ascending list of endpoints
    ///
    /// This builds an almost-complete binary search tree where the leaves represent the elementary
    /// intervals `(-infty, e0), [e0, e0], (e0, e1), ... [en, en], (en, infty)` and nodes only
    /// store information about the midpoint between left and right subtrees. The tree is
    /// represented as an array of nodes where node at index `i` has children at indices `2i + 1`
    /// and `2i + 2`.
    ///
    /// Some important facts:
    ///
    /// 1. The number of elementary intervals is always odd: `2 * n + 1` for `n` endpoints
    ///
    /// 2. With the exception of the tree that contains a single `(-infty, infty)` node, trees will
    ///    be incomplete (follows from 1 and the fact that complete trees have an even number of
    ///    leaves)
    ///
    /// 3. The number of leaves on the last row will be even and the number of leaves on the second
    ///    last row will be odd
    ///
    /// Taking this into account, we can build the almost-complete BST in reverse order: from right
    /// to left and bottom to top. We can use the above facts to figure out exactly which leaf
    /// intervals are on the last and second last levels. For the inner nodes, we maintain a queue
    /// (also in reverse order) of nodes that don't yet have a parent. We burn that queue down by
    /// popping off two items, adding a new parents, then pushing that into the queue too.
    fn build_tree(endpoints: &[Endpoint]) -> Box<[SegmentNode<Endpoint, Interval>]> {
        // Elementary intervals are: (-infty, e0), [e0, e0], (e0, e1), ... [en, en], (en, infty)
        let elementary_interval_count = 2 * endpoints.len() + 1;

        // Stores (in reverse order) the tree nodes
        let mut nodes: Vec<SegmentNode<Endpoint, Interval>> = vec![];

        // Stores (in reverse order) the intervals of nodes without a parent
        let mut inner_queue: VecDeque<BoundInterval<Endpoint>> = VecDeque::new();

        // Number of nodes on the last layer of the tree, _if it was complete_. Unless the tree has
        // a single node, this row will be incomplete.
        let n = elementary_interval_count.next_power_of_two();

        // Number of leaf nodes on the last row of the almost complete tree
        let on_last_row = 2 * elementary_interval_count - n;

        // Fill in leaf nodes in reverse order
        if on_last_row == 1 {
            nodes.push(SegmentNode::Leaf(vec![]));
            inner_queue.push_back((Bound::Unbounded, Bound::Unbounded));
        } else {
            // Last row (always even, non-empty, and never contains the final `(k, infty)` interval
            let mut idx = on_last_row / 2 - 1;
            loop {
                let endpoint = endpoints[idx];
                nodes.push(SegmentNode::Leaf(vec![]));
                nodes.push(SegmentNode::Leaf(vec![]));
                inner_queue.push_back((Bound::Included(endpoint), Bound::Included(endpoint)));
                inner_queue.push_back((
                    if idx == 0 {
                        Bound::Unbounded
                    } else {
                        Bound::Excluded(endpoints[idx - 1])
                    },
                    Bound::Excluded(endpoint),
                ));
                if idx == 0 {
                    break;
                } else {
                    idx -= 1;
                }
            }

            // Second last row (always odd, non-empty)
            let mut idx = endpoints.len() - 1;
            nodes.push(SegmentNode::Leaf(vec![]));
            inner_queue.push_back((Bound::Excluded(endpoints[idx]), Bound::Unbounded));
            while idx > on_last_row / 2 - 1 {
                let endpoint = endpoints[idx];
                nodes.push(SegmentNode::Leaf(vec![]));
                nodes.push(SegmentNode::Leaf(vec![]));
                inner_queue.push_back((Bound::Included(endpoint), Bound::Included(endpoint)));
                inner_queue.push_back((
                    Bound::Excluded(endpoints[idx - 1]),
                    Bound::Excluded(endpoint),
                ));
                idx -= 1;
            }
        }

        // Fill in the inner nodes of the tree
        while let Some((right, left)) = inner_queue.pop_front().zip(inner_queue.pop_front()) {
            // Figure out what the midpoint is and add a new inner node
            nodes.push(match left.1 {
                Bound::Included(mid) => SegmentNode::Inner {
                    mid,
                    mid_is_on_left: true,
                    intervals: vec![],
                },
                Bound::Excluded(mid) => SegmentNode::Inner {
                    mid,
                    mid_is_on_left: false,
                    intervals: vec![],
                },
                Bound::Unbounded => unreachable!(),
            });

            // Add the parent to the queue
            inner_queue.push_back((left.0, right.1));
        }

        let mut nodes = nodes.into_boxed_slice();
        nodes.reverse();
        nodes
    }

    /// Insert a new interval in the tree
    ///
    /// Invariant: the interval endpoints should already be elementary intervals!
    fn insert(&mut self, interval: Interval) {
        // Indices into the nodes vector, representing nodes to visit
        let mut to_visit: Vec<(usize, BoundInterval<Endpoint>)> = vec![];
        to_visit.push((0, (Bound::Unbounded, Bound::Unbounded)));

        while let Some((node_idx, node_interval)) = to_visit.pop() {
            // If the node's interval is fully in the input interval, add the interval to the node
            if interval_contains(&interval, &node_interval) {
                self.0[node_idx].intervals_mut().push(interval.clone());
                continue;
            }

            // Otherwise, repeat with the left and right children
            if let SegmentNode::Inner {
                mid,
                mid_is_on_left,
                ..
            } = self.0[node_idx]
            {
                // Consider the left child
                let left_interval = (
                    node_interval.0,
                    if mid_is_on_left {
                        Bound::Included(mid)
                    } else {
                        Bound::Excluded(mid)
                    },
                );
                if intervals_intersect(&left_interval, &interval) {
                    let left_child_idx = node_idx * 2 + 1;
                    to_visit.push((left_child_idx, left_interval));
                }

                // Consider the right child
                let right_interval = (
                    if mid_is_on_left {
                        Bound::Excluded(mid)
                    } else {
                        Bound::Included(mid)
                    },
                    node_interval.1,
                );
                if intervals_intersect(&right_interval, &interval) {
                    let right_child_idx = node_idx * 2 + 2;
                    to_visit.push((right_child_idx, right_interval));
                }
            }
        }
    }
}

/// Internal node in the segment tree
#[derive(Debug)]
enum SegmentNode<Endpoint, Interval> {
    /// Terminal node representing a single elementary interval (either a point or an open
    /// interval)
    Leaf(Vec<Interval>),

    /// Inner node in the tree
    Inner {
        /// Midpoint - end of the interval spanned by the left child and start of the interval
        /// spanned by the right child
        mid: Endpoint,

        /// Is the midpoint itself on the left child (or else the right child)
        mid_is_on_left: bool,

        /// Intervals contained at the node
        intervals: Vec<Interval>,
    },
}

impl<Endpoint, V> SegmentNode<Endpoint, V> {
    /// All of the intervals on this node
    fn intervals(&self) -> &[V] {
        match self {
            SegmentNode::Leaf(intervals) => intervals,
            SegmentNode::Inner { intervals, .. } => intervals,
        }
    }

    /// Add an interval to this node
    fn intervals_mut(&mut self) -> &mut Vec<V> {
        match self {
            SegmentNode::Leaf(intervals) => intervals,
            SegmentNode::Inner { intervals, .. } => intervals,
        }
    }
}

/// Does the first interval contain the second?
fn interval_contains<E: Ord, I1: RangeBounds<E>, I2: RangeBounds<E>>(
    interval1: &I1,
    interval2: &I2,
) -> bool {
    use Bound::*;

    let start1_le_start2 = match (interval1.start_bound(), interval2.start_bound()) {
        (Excluded(e1), Included(e2)) => e1 < e2,
        (Included(e1), Included(e2))
        | (Excluded(e1), Excluded(e2))
        | (Included(e1), Excluded(e2)) => e1 <= e2,
        (Unbounded, _) => true,
        (_, Unbounded) => false,
    };
    if !start1_le_start2 {
        return false;
    }

    let end2_le_end1 = match (interval2.end_bound(), interval1.end_bound()) {
        (Included(e1), Excluded(e2)) => e1 < e2,
        (Included(e1), Included(e2))
        | (Excluded(e1), Excluded(e2))
        | (Excluded(e1), Included(e2)) => e1 <= e2,
        (_, Unbounded) => true,
        (Unbounded, _) => false,
    };
    if !end2_le_end1 {
        return false;
    }

    true
}

/// Do the intervals intersect?
fn intervals_intersect<E: Ord, I1: RangeBounds<E>, I2: RangeBounds<E>>(
    interval1: &I1,
    interval2: &I2,
) -> bool {
    // Is the left endpoint less than or equal to the right endpoint
    fn left_le_right<E: Ord>(left: Bound<E>, right: Bound<E>) -> bool {
        use Bound::*;
        match (left, right) {
            (Unbounded, _) | (_, Unbounded) => true,
            (Included(e1), Included(e2)) => e1 <= e2,
            (Excluded(e1), Excluded(e2))
            | (Included(e1), Excluded(e2))
            | (Excluded(e1), Included(e2)) => e1 < e2,
        }
    }

    left_le_right(interval1.start_bound(), interval2.end_bound())
        && left_le_right(interval2.start_bound(), interval1.end_bound())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashSet;
    use std::hash::Hash;
    use std::ops::{Range, RangeInclusive};

    fn intervals_set<E: Ord + Copy, I: Hash + Eq + Clone + RangeBounds<E>>(
        tree: &SegmentTree<E, I>,
        point: E,
    ) -> HashSet<I> {
        tree.intervals_containing(&point)
            .into_iter()
            .cloned()
            .collect()
    }

    #[test]
    fn no_intervals() {
        let tree: SegmentTree<i32, RangeInclusive<i32>> = SegmentTree::new(vec![]);
        assert!(intervals_set(&tree, 0).is_empty());
        assert!(intervals_set(&tree, 1).is_empty());
        assert!(intervals_set(&tree, 2).is_empty());
    }

    #[test]
    fn single_interval() {
        let tree: SegmentTree<i32, RangeInclusive<i32>> = SegmentTree::new(vec![1..=3]);
        assert_eq!(intervals_set(&tree, 0), HashSet::from([]));
        assert_eq!(intervals_set(&tree, 1), HashSet::from([1..=3]));
        assert_eq!(intervals_set(&tree, 2), HashSet::from([1..=3]));
        assert_eq!(intervals_set(&tree, 3), HashSet::from([1..=3]));
        assert!(intervals_set(&tree, 4).is_empty());
    }

    #[test]
    fn two_overlapping_intervals() {
        let tree: SegmentTree<i32, RangeInclusive<i32>> = SegmentTree::new(vec![1..=3, 2..=4]);
        assert_eq!(intervals_set(&tree, 0), HashSet::from([]));
        assert_eq!(intervals_set(&tree, 1), HashSet::from([1..=3]));
        assert_eq!(intervals_set(&tree, 2), HashSet::from([1..=3, 2..=4]));
        assert_eq!(intervals_set(&tree, 3), HashSet::from([1..=3, 2..=4]));
        assert_eq!(intervals_set(&tree, 4), HashSet::from([2..=4]));
        assert!(intervals_set(&tree, 5).is_empty());
    }

    #[test]
    fn multiple_overlapping_intervals() {
        let tree: SegmentTree<i32, RangeInclusive<i32>> =
            SegmentTree::new(vec![0..=2, 2..=4, 4..=6, 2..=8, 0..=10]);
        assert_eq!(intervals_set(&tree, -1), HashSet::from([]));
        assert_eq!(intervals_set(&tree, 0), HashSet::from([0..=2, 0..=10]));
        assert_eq!(intervals_set(&tree, 1), HashSet::from([0..=2, 0..=10]));
        assert_eq!(
            intervals_set(&tree, 2),
            HashSet::from([0..=2, 0..=10, 2..=4, 2..=8])
        );
        assert_eq!(
            intervals_set(&tree, 3),
            HashSet::from([0..=10, 2..=4, 2..=8])
        );
        assert_eq!(
            intervals_set(&tree, 4),
            HashSet::from([0..=10, 2..=4, 2..=8, 4..=6])
        );
        assert_eq!(
            intervals_set(&tree, 5),
            HashSet::from([0..=10, 2..=8, 4..=6])
        );
        assert_eq!(
            intervals_set(&tree, 6),
            HashSet::from([0..=10, 2..=8, 4..=6])
        );
        assert_eq!(intervals_set(&tree, 7), HashSet::from([0..=10, 2..=8]));
        assert_eq!(intervals_set(&tree, 8), HashSet::from([0..=10, 2..=8]));
        assert_eq!(intervals_set(&tree, 9), HashSet::from([0..=10]));
        assert_eq!(intervals_set(&tree, 10), HashSet::from([0..=10]));
        assert_eq!(intervals_set(&tree, 11), HashSet::from([]));
    }

    #[test]
    fn multiple_overlapping_halfopen_intervals() {
        let tree: SegmentTree<i32, Range<i32>> =
            SegmentTree::new(vec![0..2, 2..4, 4..6, 2..8, 0..10]);
        assert_eq!(intervals_set(&tree, -1), HashSet::from([]));
        assert_eq!(intervals_set(&tree, 0), HashSet::from([0..2, 0..10]));
        assert_eq!(intervals_set(&tree, 1), HashSet::from([0..2, 0..10]));
        assert_eq!(intervals_set(&tree, 2), HashSet::from([0..10, 2..4, 2..8]));
        assert_eq!(intervals_set(&tree, 3), HashSet::from([0..10, 2..4, 2..8]));
        assert_eq!(intervals_set(&tree, 4), HashSet::from([0..10, 2..8, 4..6]));
        assert_eq!(intervals_set(&tree, 5), HashSet::from([0..10, 2..8, 4..6]));
        assert_eq!(intervals_set(&tree, 6), HashSet::from([0..10, 2..8]));
        assert_eq!(intervals_set(&tree, 7), HashSet::from([0..10, 2..8]));
        assert_eq!(intervals_set(&tree, 8), HashSet::from([0..10]));
        assert_eq!(intervals_set(&tree, 9), HashSet::from([0..10]));
        assert_eq!(intervals_set(&tree, 10), HashSet::from([]));
        assert_eq!(intervals_set(&tree, 11), HashSet::from([]));
    }
}
