use nabla_game;
use nabla_game::basis::structs::*;
use nabla_game::math::integral::integral;

pub mod util;
use util::*;

// test log integrals
#[test]
fn test_log_integral() {
    let (mut a, mut b);

    // integral of log(x)
    a = log_x();
    b = Basis::x() * log_x() - Basis::x();
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // integral of 1/x
    a = Basis::x() ^ -1;
    b = log_x();
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // integral of xlog(x)
    a = Basis::x() * log_x();
    b = (2 * (Basis::x() ^ 2) * log_x() - (Basis::x() ^ 2)) / 4;
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // integral of log(x)/x
    a = log_x() / Basis::x();
    b = (log_x() ^ 2) / 2;
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // integral of log(x)/x^2
    a = log_x() / (Basis::x() ^ 2);
    b = -log_x() / Basis::x() - (Basis::x() ^ -1);
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // ! needs distributive property
    // // integral of log^2(x)
    // a = log_x() * log_x();
    // b = Basis::x() * (log_x() ^ 2) - 2 * Basis::x() * log_x() + 2 * Basis::x();
    // println!("I({}) = {}", a, b);
    // println!("{} = {}", integral(&a), b);
    // assert_eq!(integral(&a), b);
}

// test tabular integration
#[test]
fn test_tabular_integration() {
    let (mut a, mut b);

    // integral of xsin(x)
    a = sin_x() * Basis::x();
    b = sin_x() - Basis::x() * cos_x();
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // integral of x^2cos(x)
    a = (Basis::x() ^ 2) * cos_x();
    b = ((Basis::x() ^ 2) * sin_x()) + (2 * Basis::x() * cos_x()) - (2 * sin_x());
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);

    // integral of x^3e^x
    a = (Basis::x() ^ 3) * e_x();
    b = ((Basis::x() ^ 3) * e_x()) - (3 * (Basis::x() ^ 2) * e_x()) + (6 * Basis::x() * e_x())
        - (6 * e_x());
    println!("I({}) = {}", a, b);
    assert_eq!(integral(&a), b);
}

/// reproduces a reported freeze: playing Integral against x^2 * sqrt(log(cos(x^2)))
/// (a real field state from a match report) took over a minute without
/// returning. Tabular integration by parts (see tabular_integration in
/// src/math/integral.rs) assumes dv's repeated integrals stay about as simple
/// as dv itself -- true for its intended targets (sin(x)/cos(x)/e^x, all size
/// 2), but sqrt(log(cos(x^2))) is not one of those, so each successive
/// integral() call in its loop operated on an already-more-complex result,
/// compounding into an exponential blowup that never panics or exceeds
/// ComputeDepthGuard's recursion-depth cap (it's iterative growth across loop
/// iterations, not recursion depth). This only checks that integral() returns
/// promptly -- not what it returns, since falling back to a symbolic
/// placeholder rather than a fully expanded closed form is the whole point of
/// the fix
#[test]
fn test_tabular_integration_does_not_hang_on_a_non_elementary_dv() {
    let pathological = (Basis::x() ^ 2) * ((log(&cos(&(Basis::x() ^ 2)))) ^ (1, 2));

    let start = std::time::Instant::now();
    let _ = integral(&pathological);
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "integral() took {:?} against a non-elementary tabular dv -- too slow, risks looking frozen",
        elapsed
    );
}
