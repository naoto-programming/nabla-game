use std::collections::HashMap;

use nabla_game;
use nabla_game::basis::builders::ACosBasisNode;
use nabla_game::basis::structs::*;
use nabla_game::math::derivative::derivative;

pub mod util;
use util::*;

// test basic derivatives
#[test]
fn test_basic_derivatives() {
    let derivative_lookup = HashMap::from([
        (Basis::from(0), Basis::from(0)),
        (Basis::from(1), Basis::from(0)),
        (Basis::x(), Basis::from(1)),
        (Basis::x() ^ 2, 2 * Basis::x()),
        (e_x(), e_x()),
        (log_x(), Basis::x() ^ -1),
        (cos_x(), -sin_x()),
        (sin_x(), cos_x()),
    ]);

    for (key, value) in derivative_lookup.into_iter() {
        println!("d/dx({}) = {}", key, value);
        assert_eq!(derivative(&key), value);
    }
}

// test Add and Minus derivatives
#[test]
fn test_add_derivatives() {
    let (mut a, mut b);

    // test first derivative
    a = Basis::x() + e_x();
    b = Basis::from(1) + e_x();
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test second derivative
    a = cos_x() + (Basis::x() ^ 2);
    b = -cos_x() + Basis::from(2);
    println!("d/dx(d/dx({})) = {}", a, b);
    assert_eq!(derivative(&derivative(&a)), b);

    // test trinomial (nested BasisNode)
    a = sin_x() + (Basis::x() ^ 2) + Basis::x();
    b = cos_x() + (Basis::x() * 2) + Basis::from(1);
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test trim 0
    a = cos_x() + Basis::from(1);
    b = -sin_x();
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);
}

// test Mult and Div derivatives
#[test]
fn test_mult_derivatives() {
    let (mut a, mut b);

    // test mult derivative
    a = (Basis::x() ^ 2) * cos_x();
    b = ((Basis::x() ^ 2) * -sin_x()) + (cos_x() * Basis::x() * 2);
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test trim 1
    a = Basis::x() * e_x();
    b = (Basis::x() * e_x()) + e_x();
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test div derivative
    a = cos_x() / e_x();
    b = ((-sin_x() * e_x()) - (cos_x() * e_x())) / (e_x() ^ 2);
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);
}

// test Pow and Sqrt derivatives
#[test]
fn test_exponent_derivatives() {
    let (mut a, mut b);

    // test pow derivative
    a = Basis::x() ^ 4;
    b = 4 * (Basis::x() ^ 3);
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test sqrt derivative
    a = Basis::x() ^ (1, 2);
    b = (Basis::x() ^ (-1, 2)) / 2;
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test sin pow derivative
    a = cos_x() ^ 2;
    b = -2 * sin_x() * cos_x();
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test e pow derivative
    a = e_x() ^ (1, 2);
    b = e(&(Basis::x() / 2)) / 2;
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);
}

// test Log derivatives
#[test]
fn test_log_derivatives() {
    let (mut a, mut b);

    // test log(x) derivative
    a = log_x();
    b = Basis::x() ^ -1;
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test log(sin(x)) derivative
    a = log(&sin_x());
    b = cos_x() / sin_x();
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);

    // test log(acos(x)) derivative
    a = log(&ACosBasisNode(&Basis::x()));
    b = derivative(&ACosBasisNode(&Basis::x())) / ACosBasisNode(&Basis::x());
    println!("d/dx({}) = {}", a, b);
    assert_eq!(derivative(&a), b);
}

/// reproduces a reported multi-second AI decision delay: playing Inverse
/// against a field slot the game couldn't cleanly invert (eg. sin(x)/x) wraps
/// it as InvBasisNode(sin(x)/x) -- inverse()'s "give up" placeholder for an
/// operator with no known symbolic inverse, not a genuine functional inverse.
/// Differentiating that used to run the real inverse-function-rule formula
/// (1/f'(f-1(x))) via function_composition regardless, substituting the WHOLE
/// wrapped expression into every x in 1/f'(x); harmless for a bare-x operand
/// (the only case the rule is actually meaningful for), but the result's size
/// multiplies with the operand's size for anything else -- and compounds
/// further if the AI's lookahead differentiates an already-stacked or
/// already-differentiated Inv placeholder again. It should just return the
/// input unchanged instead
#[test]
fn test_inv_derivative_of_a_non_trivial_placeholder_is_fast_and_unchanged() {
    use nabla_game::basis::builders::InvBasisNode;

    let non_trivial = InvBasisNode(&(sin_x() / Basis::x()));

    let start = std::time::Instant::now();
    let result = derivative(&non_trivial);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "derivative() took {:?} against a non-trivial Inv placeholder -- too slow, risks looking frozen",
        elapsed
    );
    assert_eq!(result, non_trivial, "should bail out to the unchanged input");
}
