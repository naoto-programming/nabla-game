// std imports
use std::cell::Cell;
// outer crate imports
use crate::basis::structs::*;

/// tracks recursive depth across derivative()/integral(), which call each other
/// (eg. integration by parts, inverse rule) and can otherwise recurse unboundedly
/// on some expressions, freezing or crashing the tab
/// (thread-local rather than a shared static: the game itself is single-threaded
/// wasm, but `cargo test` runs tests concurrently on separate threads)
thread_local! {
    static COMPUTE_DEPTH: Cell<u32> = Cell::new(0);
}
const MAX_COMPUTE_DEPTH: u32 = 48;

/// RAII guard incrementing COMPUTE_DEPTH on creation, decrementing on drop;
/// `enter()` returns None once MAX_COMPUTE_DEPTH is exceeded so callers can bail out
pub struct ComputeDepthGuard;
impl ComputeDepthGuard {
    pub fn enter() -> Option<Self> {
        let entered = COMPUTE_DEPTH.with(|depth| {
            if depth.get() >= MAX_COMPUTE_DEPTH {
                return false;
            }
            depth.set(depth.get() + 1);
            true
        });
        if entered {
            Some(ComputeDepthGuard)
        } else {
            None
        }
    }
}
impl Drop for ComputeDepthGuard {
    fn drop(&mut self) {
        COMPUTE_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

/// implements recursive f o g
pub fn function_composition(f: &Basis, g: &Basis) -> Basis {
    match f.clone() {
        Basis::BasisLeaf(basis_leaf) => {
            if basis_leaf.element == BasisElement::X {
                g.clone() * basis_leaf.coefficient
            } else {
                f.clone()
            }
        }
        Basis::BasisNode(basis_node) => Basis::BasisNode(BasisNode {
            operands: basis_node
                .operands
                .iter()
                .map(|op| function_composition(op, g))
                .collect(),
            ..basis_node
        }),
    }
}
