use crate::merkle::MerkleItem;
use serde::{Deserialize, Serialize};

use super::super::encoding::{self, Encodable};
use super::nodes::{Hash, NodeHasher};

/// Absolute position of an item in the tree.
pub type Position = u64;

/// Merkle proof of inclusion of a node in a `Forest`.
/// The exact tree is determined by the `position`, an absolute position of the item
/// within the set of all items in the forest.
/// Neighbors are counted from lowest to the highest.
/// Left/right position of the neighbor is determined by the appropriate bit in `position`.
/// (Lowest bit=1 means the first neighbor is to the left of the node.)
/// `generation` points to the generation of the Forest to which the proof applies.
/// `path` is None if this proof is for a newly added item that has no merkle path yet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proof {
    /// Generation of the forest to which the proof applies.
    pub generation: u64,

    /// Merkle path to the item. If missing, the proof applies to a yet-to-be-normalized forest.
    pub path: Path,
}

/// Merkle path to the item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Path {
    pub(super) position: Position,
    pub(super) neighbors: Vec<Hash>,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub(super) enum Side {
    Left,
    Right,
}

impl Side {
    /// Orders (current, neighbor) pair of nodes as (left, right) per `current`'s side.
    pub(super) fn order<T>(self, node: T, neighbor: T) -> (T, T) {
        match self {
            Side::Left => (node, neighbor),
            Side::Right => (neighbor, node),
        }
    }

    /// Returns (current, neighbor) pair, reversing effects of `order`.
    pub(super) fn choose<T>(self, left: T, right: T) -> (T, T) {
        match self {
            Side::Left => (left, right),
            Side::Right => (right, left),
        }
    }

    fn from_bit(bit: u8) -> Self {
        match bit {
            0 => Side::Left,
            _ => Side::Right,
        }
    }
}

impl Path {
    pub(super) fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (Side, &Hash)> + ExactSizeIterator {
        self.directions().zip(self.neighbors.iter())
    }
    pub(super) fn directions(&self) -> Directions {
        Directions {
            position: self.position,
            depth: self.neighbors.len(),
        }
    }
    /// Returns an iterator that walks up the path
    /// and yields parent hash and children hashes at each step.
    pub(super) fn walk_up<'a, 'b: 'a, M: MerkleItem>(
        &'a self,
        item_hash: Hash,
        hasher: &'b NodeHasher<M>,
    ) -> impl Iterator<Item = (Hash, (Hash, Hash))> + 'a {
        self.iter()
            .scan(item_hash, move |item_hash, (side, neighbor)| {
                let (l, r) = side.order(*item_hash, *neighbor);
                let p = hasher.intermediate(&l, &r);
                *item_hash = p;
                Some((p, (l, r)))
            })
    }
}

impl Encodable for Proof {
    fn encode(&self, buf: &mut Vec<u8>) {
        encoding::write_u64(self.generation, buf);
        self.path.encode(buf);
    }

    fn serialized_length(&self) -> usize {
        8 + self.path.serialized_length()
    }
}

impl Encodable for Path {
    fn encode(&self, buf: &mut Vec<u8>) {
        encoding::write_u64(self.position, buf);
        encoding::write_size(self.neighbors.len(), buf);
        for hash in self.neighbors.iter() {
            encoding::write_bytes(&hash[..], buf);
        }
    }

    fn serialized_length(&self) -> usize {
        return 8 + 4 + 32 * self.neighbors.len();
    }
}

/// Simialr to Path, but does not contain neighbors - only left/right directions
/// as indicated by the bits in the `position`.
#[derive(Copy, Clone, PartialEq, Debug)]
pub(super) struct Directions {
    pub(super) position: Position,
    pub(super) depth: usize,
}

impl ExactSizeIterator for Directions {
    fn len(&self) -> usize {
        self.depth
    }
}

impl Iterator for Directions {
    type Item = Side;
    fn next(&mut self) -> Option<Self::Item> {
        if self.depth == 0 {
            return None;
        }
        let side = Side::from_bit((self.position & 1) as u8);
        // kick out the lowest bit and shrink the depth
        self.position >>= 1;
        self.depth -= 1;
        Some(side)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl DoubleEndedIterator for Directions {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        // Note: we do not mask out the bit in `position` because we don't expose it.
        // The bit is ignored implicitly by having the depth decremented.
        let side = Side::from_bit(((self.position >> self.depth) & 1) as u8);
        Some(side)
    }
}
