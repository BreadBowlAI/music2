//! One floating-point mode on every thread that renders (RENDER-2a).

use super::*;

/// A sine through `std:scale` by 1e-30, then 1e-10 (about 1e-40: subnormal), then 1e30 and 1e10:
/// near the sine again where subnormals are kept, silence where they are flushed to zero.
pub(super) fn subnormal_chain() -> Ir {
    let mut nodes = vec![
        json!({"id": "f", "op": "const", "value": 440, "unit": "hz"}),
        json!({"id": "x0", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}),
    ];
    for (k, by) in [1e-30, 1e-10, 1e30, 1e10].into_iter().enumerate() {
        nodes.push(json!({"id": format!("by{k}"), "op": "const", "value": by}));
        nodes.push(json!({"id": format!("x{}", k + 1), "op": "kernel", "kernel": "std:scale", "inputs": {"in": format!("x{k}"), "by": format!("by{k}")}}));
    }
    song(Value::Array(nodes), "x4", json!([]))
}

// RENDER-2a: a thread that flushes subnormals to zero (a host's, a device's) renders the same bits
// as one that keeps them, and flushes again afterwards: the engine sets the one mode for the
// block and puts the thread's own back.
#[test]
fn a_flush_to_zero_set_by_the_caller_changes_no_bit() {
    let pk = std_packages();
    let f = check(&subnormal_chain(), &pk).unwrap();
    let kept = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.1).unwrap();
    assert!(peak(&kept) > 0.5, "the chain gives the sine back: {}", peak(&kept));
    let (flushed, after) = std::thread::scope(|s| {
        s.spawn(|| {
            crate::fpenv::flush_to_zero();
            let out = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(0.1).unwrap();
            (out, crate::fpenv::flushes())
        })
        .join()
        .unwrap()
    });
    assert!(flushed == kept, "the same bits on a thread that flushes");
    assert!(after, "the thread flushes again afterwards");
}
