//! Directed graph algorithms on dense `usize` vertex ids.
//!
//! | operation                    | algorithm                         | complexity |
//! |------------------------------|-----------------------------------|------------|
//! | [`DiGraph::would_create_cycle`] | iterative DFS reachability     | O(V + E)   |
//! | [`DiGraph::topo_sort`]       | Kahn's algorithm                  | O(V + E)   |
//! | [`DiGraph::strongly_connected`] | Tarjan (iterative)             | O(V + E)   |
//! | [`DiGraph::levels`]          | longest path from roots via Kahn  | O(V + E)   |
//! | [`DiGraph::critical_path`]   | DP over topological order         | O(V + E)   |
//! | [`DiGraph::remaining_depth`] | DP over reverse topological order | O(V + E)   |
//! | [`DiGraph::weak_components`] | union–find                        | O(E·α(V))  |

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use super::union_find::UnionFind;

/// Adjacency-list digraph with `n` vertices `0..n`.
#[derive(Debug, Clone, Default)]
pub struct DiGraph {
    out: Vec<Vec<usize>>,
    inc: Vec<Vec<usize>>,
}

impl DiGraph {
    /// Graph with `n` isolated vertices.
    pub fn new(n: usize) -> Self {
        DiGraph {
            out: vec![Vec::new(); n],
            inc: vec![Vec::new(); n],
        }
    }

    /// Number of vertices.
    pub fn len(&self) -> usize {
        self.out.len()
    }

    /// True when the graph has no vertices.
    pub fn is_empty(&self) -> bool {
        self.out.is_empty()
    }

    /// Adds the edge `s → t` (parallel edges are ignored).
    pub fn add_edge(&mut self, s: usize, t: usize) {
        if !self.out[s].contains(&t) {
            self.out[s].push(t);
            self.inc[t].push(s);
        }
    }

    /// Direct successors of `v`.
    pub fn successors(&self, v: usize) -> &[usize] {
        &self.out[v]
    }

    /// Direct predecessors of `v`.
    pub fn predecessors(&self, v: usize) -> &[usize] {
        &self.inc[v]
    }

    /// True when `from` can reach `to` along directed edges.
    pub fn reaches(&self, from: usize, to: usize) -> bool {
        let mut seen = vec![false; self.len()];
        let mut stack = vec![from];
        while let Some(v) = stack.pop() {
            if v == to {
                return true;
            }
            if std::mem::replace(&mut seen[v], true) {
                continue;
            }
            stack.extend(self.out[v].iter().copied().filter(|&w| !seen[w]));
        }
        false
    }

    /// True when adding `s → t` would close a cycle (i.e. `t` reaches `s`).
    pub fn would_create_cycle(&self, s: usize, t: usize) -> bool {
        s == t || self.reaches(t, s)
    }

    /// Kahn's algorithm. Returns the topological order of all vertices, or
    /// `Err(order)` with the order of the acyclic part when a cycle exists.
    pub fn topo_sort(&self) -> Result<Vec<usize>, Vec<usize>> {
        let mut indeg: Vec<usize> = self.inc.iter().map(Vec::len).collect();
        let mut queue: VecDeque<usize> = (0..self.len()).filter(|&v| indeg[v] == 0).collect();
        let mut order = Vec::with_capacity(self.len());
        while let Some(v) = queue.pop_front() {
            order.push(v);
            for &w in &self.out[v] {
                indeg[w] -= 1;
                if indeg[w] == 0 {
                    queue.push_back(w);
                }
            }
        }
        if order.len() == self.len() {
            Ok(order)
        } else {
            Err(order)
        }
    }

    /// Tarjan's strongly connected components (iterative, no recursion).
    /// Components are returned in reverse topological order of the condensation.
    pub fn strongly_connected(&self) -> Vec<Vec<usize>> {
        const UNVISITED: usize = usize::MAX;
        let n = self.len();
        let (mut index, mut low) = (vec![UNVISITED; n], vec![0; n]);
        let mut on_stack = vec![false; n];
        let (mut stack, mut result) = (Vec::new(), Vec::new());
        let mut next_index = 0;
        for root in 0..n {
            if index[root] != UNVISITED {
                continue;
            }
            // Call stack of (vertex, next successor position).
            let mut call = vec![(root, 0usize)];
            index[root] = next_index;
            low[root] = next_index;
            next_index += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(&mut (v, ref mut pos)) = call.last_mut() {
                if let Some(&w) = self.out[v].get(*pos) {
                    *pos += 1;
                    if index[w] == UNVISITED {
                        index[w] = next_index;
                        low[w] = next_index;
                        next_index += 1;
                        stack.push(w);
                        on_stack[w] = true;
                        call.push((w, 0));
                    } else if on_stack[w] {
                        low[v] = low[v].min(index[w]);
                    }
                    continue;
                }
                call.pop();
                if let Some(&(parent, _)) = call.last() {
                    low[parent] = low[parent].min(low[v]);
                }
                if low[v] == index[v] {
                    let mut component = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        component.push(w);
                        if w == v {
                            break;
                        }
                    }
                    result.push(component);
                }
            }
        }
        result
    }

    /// Components that form cycles: SCCs with more than one vertex, or a self loop.
    pub fn cycles(&self) -> Vec<Vec<usize>> {
        self.strongly_connected()
            .into_iter()
            .filter(|c| c.len() > 1 || self.out[c[0]].contains(&c[0]))
            .collect()
    }

    /// Parallel execution waves of the acyclic part: a vertex's level is the
    /// length of the longest path from any root to it.
    pub fn levels(&self) -> Vec<Vec<usize>> {
        let order = self.topo_sort().unwrap_or_else(|partial| partial);
        let mut level = vec![0usize; self.len()];
        let mut waves: Vec<Vec<usize>> = Vec::new();
        for &v in &order {
            level[v] = self.inc[v].iter().map(|&u| level[u] + 1).max().unwrap_or(0);
            if waves.len() <= level[v] {
                waves.resize_with(level[v] + 1, Vec::new);
            }
            waves[level[v]].push(v);
        }
        waves
    }

    /// The longest path (by vertex count) through the acyclic part.
    pub fn critical_path(&self) -> Vec<usize> {
        let order = self.topo_sort().unwrap_or_else(|partial| partial);
        let mut dist = vec![0usize; self.len()];
        let mut prev = vec![None; self.len()];
        for &v in &order {
            for &w in &self.out[v] {
                if dist[v] + 1 > dist[w] {
                    dist[w] = dist[v] + 1;
                    prev[w] = Some(v);
                }
            }
        }
        let Some(mut end) = order
            .iter()
            .copied()
            .max_by_key(|&v| (dist[v], std::cmp::Reverse(v)))
        else {
            return Vec::new();
        };
        let mut path = vec![end];
        while let Some(p) = prev[end] {
            path.push(p);
            end = p;
        }
        path.reverse();
        path
    }

    /// For every vertex, the number of vertices on the longest path starting
    /// at it (1 for sinks). Used as the scheduler's critical-path priority.
    pub fn remaining_depth(&self) -> Vec<u32> {
        let order = self.topo_sort().unwrap_or_else(|partial| partial);
        let mut depth = vec![1u32; self.len()];
        for &v in order.iter().rev() {
            depth[v] = 1 + self.out[v].iter().map(|&w| depth[w]).max().unwrap_or(0);
        }
        depth
    }

    /// All vertices reachable from `v` (excluding `v`).
    pub fn descendants(&self, v: usize) -> Vec<usize> {
        let mut seen = vec![false; self.len()];
        let mut stack = self.out[v].clone();
        let mut out = Vec::new();
        while let Some(w) = stack.pop() {
            if std::mem::replace(&mut seen[w], true) {
                continue;
            }
            out.push(w);
            stack.extend(self.out[w].iter().copied().filter(|&x| !seen[x]));
        }
        out
    }

    /// Weakly connected components (edge direction ignored), each sorted,
    /// ordered by their smallest vertex.
    pub fn weak_components(&self) -> Vec<Vec<usize>> {
        let mut uf = UnionFind::new(self.len());
        for (s, targets) in self.out.iter().enumerate() {
            for &t in targets {
                uf.union(s, t);
            }
        }
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for v in 0..self.len() {
            groups.entry(uf.find(v)).or_default().push(v);
        }
        let mut comps: Vec<Vec<usize>> = groups.into_values().collect();
        comps.sort_by_key(|c| c[0]);
        comps
    }
}

/// Maps arbitrary keys (e.g. node ids) to dense vertex indices.
#[derive(Debug, Clone)]
pub struct Indexed<K> {
    keys: Vec<K>,
    index: HashMap<K, usize>,
}

impl<K: Copy + Eq + Hash> Indexed<K> {
    /// Indexes `keys` in order (duplicates keep their first index).
    pub fn new(keys: impl IntoIterator<Item = K>) -> Self {
        let mut out = Indexed {
            keys: Vec::new(),
            index: HashMap::new(),
        };
        for k in keys {
            if !out.index.contains_key(&k) {
                out.index.insert(k, out.keys.len());
                out.keys.push(k);
            }
        }
        out
    }

    /// Vertex index of `key`.
    pub fn get(&self, key: &K) -> Option<usize> {
        self.index.get(key).copied()
    }

    /// Key of vertex `i`.
    pub fn key(&self, i: usize) -> K {
        self.keys[i]
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// True when no key is indexed.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Builds a [`DiGraph`] from key pairs, skipping pairs with unknown keys.
    pub fn graph(&self, edges: impl IntoIterator<Item = (K, K)>) -> DiGraph {
        let mut g = DiGraph::new(self.len());
        for (s, t) in edges {
            if let (Some(s), Some(t)) = (self.get(&s), self.get(&t)) {
                g.add_edge(s, t);
            }
        }
        g
    }

    /// Translates vertex indices back to keys.
    pub fn keys_of(&self, vs: &[usize]) -> Vec<K> {
        vs.iter().map(|&v| self.key(v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0 → 1 → 3, 0 → 2 → 3, 3 → 4, plus isolated 5.
    fn diamond() -> DiGraph {
        let mut g = DiGraph::new(6);
        for (s, t) in [(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)] {
            g.add_edge(s, t);
        }
        g
    }

    #[test]
    fn topo_sort_respects_edges() {
        let g = diamond();
        let order = g.topo_sort().unwrap();
        let pos = |v| order.iter().position(|&x| x == v).unwrap();
        for v in 0..g.len() {
            for &w in g.successors(v) {
                assert!(pos(v) < pos(w));
            }
        }
    }

    #[test]
    fn detects_cycles() {
        let mut g = diamond();
        assert!(g.would_create_cycle(4, 0));
        assert!(g.would_create_cycle(2, 2));
        assert!(!g.would_create_cycle(0, 4));
        assert!(g.cycles().is_empty());
        g.add_edge(4, 1);
        assert!(g.topo_sort().is_err());
        let cycles = g.cycles();
        assert_eq!(cycles.len(), 1);
        let mut c = cycles[0].clone();
        c.sort();
        assert_eq!(c, vec![1, 3, 4]);
    }

    #[test]
    fn levels_and_critical_path() {
        let g = diamond();
        assert_eq!(g.levels(), vec![vec![0, 5], vec![1, 2], vec![3], vec![4]]);
        assert_eq!(g.critical_path(), vec![0, 1, 3, 4]);
        assert_eq!(g.remaining_depth(), vec![4, 3, 3, 2, 1, 1]);
        assert!(DiGraph::new(0).critical_path().is_empty());
    }

    #[test]
    fn descendants_and_components() {
        let g = diamond();
        let mut d = g.descendants(1);
        d.sort();
        assert_eq!(d, vec![3, 4]);
        assert_eq!(g.weak_components(), vec![vec![0, 1, 2, 3, 4], vec![5]]);
        assert_eq!(g.predecessors(3), &[1, 2]);
    }

    #[test]
    fn tarjan_handles_long_chains_without_recursion() {
        let n = 100_000;
        let mut g = DiGraph::new(n);
        for v in 0..n - 1 {
            g.add_edge(v, v + 1);
        }
        g.add_edge(n - 1, 0);
        assert_eq!(g.cycles().len(), 1);
    }

    #[test]
    fn indexed_maps_keys() {
        let idx = Indexed::new(["a", "b", "c", "a"]);
        assert_eq!(idx.len(), 3);
        let g = idx.graph([("a", "b"), ("b", "c"), ("x", "a")]);
        assert_eq!(idx.keys_of(&g.topo_sort().unwrap()), vec!["a", "b", "c"]);
    }
}
