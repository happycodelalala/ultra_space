//! The shape set and the slope rule (block-world.md section 3).
//!
//! A shaped cell is a block whose top, along one of six "up" directions, has its four corners at
//! full or zero height: two down is a 45-degree wedge, three down an outer corner, one down a
//! valley corner. The rule: a cell open above slopes toward a neighbour that is empty with solid
//! below it (a step one block high) when the solid continues behind the cell, so one- and
//! two-block bumps stay blocks; a cell whose two neighbours both slope toward their shared
//! diagonal becomes a valley corner. A single slope also needs a solid neighbour along its edge. A model's recipe lists the ups it smooths, in priority order.

use std::collections::HashMap;

use glam::IVec3;

use crate::grid::Cells;

/// One of the six axis directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Dir {
    pub axis: u8,
    pub sign: i8,
}

impl Dir {
    pub const fn new(axis: u8, sign: i8) -> Self {
        Self { axis, sign }
    }
    pub const PX: Dir = Dir::new(0, 1);
    pub const NX: Dir = Dir::new(0, -1);
    pub const PY: Dir = Dir::new(1, 1);
    pub const NY: Dir = Dir::new(1, -1);
    pub const PZ: Dir = Dir::new(2, 1);
    pub const NZ: Dir = Dir::new(2, -1);

    pub fn vec(self) -> IVec3 {
        let mut v = IVec3::ZERO;
        v[self.axis as usize] = self.sign as i32;
        v
    }

    pub fn flip(self) -> Dir {
        Dir::new(self.axis, -self.sign)
    }

    /// Parses "+z", "-x", ...
    pub fn parse(s: &str) -> Option<Dir> {
        let sign = match s.as_bytes().first()? {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let axis = match s.get(1..)? {
            "x" => 0,
            "y" => 1,
            "z" => 2,
            _ => return None,
        };
        Some(Dir::new(axis, sign))
    }

    /// The two axes across this one, in increasing order (the shape's p and q).
    pub fn across(self) -> (usize, usize) {
        match self.axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        }
    }
}

/// The top corners in (p, q) order: (0,0), (1,0), (1,1), (0,1).
pub const RING: [(i32, i32); 4] = [(0, 0), (1, 0), (1, 1), (0, 1)];

/// A shaped cell: its up direction and which of its four top corners are at full height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Shape {
    pub up: Dir,
    pub high: [bool; 4],
}

impl Shape {
    pub fn highs(&self) -> usize {
        self.high.iter().filter(|&&h| h).count()
    }

    /// Whether this cell's face toward `d` covers the whole square (so a neighbour's face against
    /// it is hidden).
    pub fn face_full(&self, d: Dir) -> bool {
        if d == self.up.flip() {
            return true;
        }
        if d == self.up {
            return false;
        }
        let (p, _) = self.up.across();
        // The two ring corners on side d.
        let side = if d.axis as usize == p { 0 } else { 1 };
        let at = if d.sign > 0 { 1 } else { 0 };
        RING.iter()
            .enumerate()
            .filter(|(_, c)| (if side == 0 { c.0 } else { c.1 }) == at)
            .all(|(k, _)| self.high[k])
    }
}

fn add(p: IVec3, d: Dir) -> IVec3 {
    p + d.vec()
}

/// Shapes for the solid cells in [`lo`, `hi`) by the slope rule over `ups` (priority order).
pub fn shapes(cells: &impl Cells, lo: IVec3, hi: IVec3, ups: &[Dir]) -> HashMap<IVec3, Shape> {
    let mut out: HashMap<IVec3, Shape> = HashMap::new();
    for &up in ups {
        let (pa, qa) = up.across();
        let dirs = [
            Dir::new(pa as u8, 1),
            Dir::new(pa as u8, -1),
            Dir::new(qa as u8, 1),
            Dir::new(qa as u8, -1),
        ];
        let down = up.flip();
        let low = |c: IVec3, d: Dir| {
            let n = add(c, d);
            !cells.solid(n) && cells.solid(add(n, down))
        };
        for z in lo.z..hi.z {
            for y in lo.y..hi.y {
                for x in lo.x..hi.x {
                    let c = IVec3::new(x, y, z);
                    if !cells.solid(c) || out.contains_key(&c) || cells.solid(add(c, up)) {
                        continue;
                    }
                    let lows: Vec<Dir> = dirs
                        .iter()
                        .copied()
                        .filter(|&d| low(c, d) && cells.solid(add(c, d.flip())))
                        .collect();
                    let mut high = [true; 4];
                    // A single slope needs a neighbour along its edge, so a ridge one block
                    // wide stays a block.
                    let along = |d: Dir| {
                        let r = if d.axis as usize == pa { qa } else { pa };
                        cells.solid(add(c, Dir::new(r as u8, 1)))
                            || cells.solid(add(c, Dir::new(r as u8, -1)))
                    };
                    let shaped = if (lows.len() == 1 && along(lows[0]))
                        || (lows.len() == 2 && lows[0].axis != lows[1].axis)
                    {
                        for (k, &(cp, cq)) in RING.iter().enumerate() {
                            for d in &lows {
                                let cc = if d.axis as usize == pa { cp } else { cq };
                                if (d.sign > 0 && cc == 1) || (d.sign < 0 && cc == 0) {
                                    high[k] = false;
                                }
                            }
                        }
                        true
                    } else if lows.is_empty() {
                        let mut found = false;
                        'v: for &pd in &dirs[..2] {
                            for &qd in &dirs[2..] {
                                let (n1, n2) = (add(c, pd), add(c, qd));
                                let dg = add(n1, qd);
                                if cells.solid(n1)
                                    && cells.solid(n2)
                                    && !cells.solid(dg)
                                    && cells.solid(add(dg, down))
                                    && !cells.solid(add(n1, up))
                                    && !cells.solid(add(n2, up))
                                    && low(n1, qd)
                                    && low(n2, pd)
                                {
                                    for (k, &(cp, cq)) in RING.iter().enumerate() {
                                        if cp == i32::from(pd.sign > 0)
                                            && cq == i32::from(qd.sign > 0)
                                        {
                                            high[k] = false;
                                        }
                                    }
                                    found = true;
                                    break 'v;
                                }
                            }
                        }
                        found
                    } else {
                        false
                    };
                    if shaped {
                        out.insert(c, Shape { up, high });
                    }
                }
            }
        }
    }
    out
}
