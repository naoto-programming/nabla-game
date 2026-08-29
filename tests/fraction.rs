use nabla_game;
use nabla_game::basis::builders::SqrtBasisNode;
use nabla_game::basis::structs::Basis;
use nabla_game::math::derivative::derivative;
use nabla_game::math::fraction::Fraction;

/// Fraction::simplify previously bailed out on a zero numerator without
/// normalizing the denominator to 1, so eg. {0,2} (produced whenever a
/// derivative's legitimately-zero factor gets scaled by a coefficient like
/// 1/2) survived as a distinct, non-canonical value from {0,1}. Every
/// zero-check elsewhere (Fraction's PartialEq<i32>, Basis::is_num,
/// AddBasisNode's own zero filter) requires the denominator to already be 1,
/// so a {0,2} silently evaded all of them
#[test]
fn test_simplify_normalizes_zero_numerator_denominator_to_one() {
    let non_canonical_zero = Fraction { n: 0, d: 2 };
    assert_eq!(non_canonical_zero.simplify(), Fraction { n: 0, d: 1 });
}

#[test]
fn test_multiplying_by_a_fraction_never_produces_a_non_canonical_zero() {
    let zero = Fraction::from(0);
    let half = Fraction::from((1, 2));
    assert_eq!(zero * half, Fraction { n: 0, d: 1 });
    assert_eq!(half * zero, Fraction { n: 0, d: 1 });
}

/// reproduces a real reported loss: playing Laplacian (double derivative) on
/// 2^(1/2) * x^(1/2) produced "(2^(1/2))/(2x^(1/2)) + (0/2) * x^(1/2)" -- the
/// second term's coefficient should have collapsed to plain zero and been
/// dropped by AddBasisNode's zero filter, but the filter's equality check
/// requires a canonical {0,1} and never matched the non-canonical {0,2} this
/// produced. Left uncaught, the AI kept re-differentiating/re-integrating
/// this phantom term turn after turn, and the expression grew unboundedly
#[test]
fn test_double_derivative_of_sqrt_2x_does_not_leave_a_zero_numerator_term() {
    let sqrt_2x = SqrtBasisNode(1, &Basis::x().with_coefficient(2));
    let once = derivative(&sqrt_2x);
    let twice = derivative(&once);
    let rendered = twice.to_string();
    assert!(
        !rendered.contains("(0/"),
        "expected the zero-coefficient term to be dropped entirely rather than \
         linger as a visible zero-numerator fraction, got: {}",
        rendered
    );
}
