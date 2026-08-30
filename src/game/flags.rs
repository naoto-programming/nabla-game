/// used in LaTeX formatting to use LN instead of LOG for the natural logarithm
pub static mut DISPLAY_LN_FOR_LOG: bool = false;
/// allow multiple field Basis with same base but different coefficients
pub static mut ALLOW_LINEAR_DEPENDENCE: bool = false;
/// allow limits of inverse trigonometric functions beyond the range of the function (ie. lim→INF)
/// 0 = disabled, 1 = enabled, 2 = range selection mode
pub static mut ALLOW_LIMITS_BEYOND_BOUNDS: u8 = 1;
/// principal value selection for inverse trig functions (when mode is 2)
/// 0 = standard [0, π] for arccos, [-π/2, π/2] for arcsin
/// 1 = alternative ranges
pub static mut INVERSE_TRIG_PRINCIPAL_VALUE: u8 = 0;
/// fully expand all functions (ie. integrals, inverse)
pub static mut FULL_COMPUTE: bool = false;
/// display fractional exponents as a rational number or with nth root notation
pub static mut USE_FRACTIONAL_EXPONENTS: bool = true;
/// restrict field to maximum 3 Basis
pub static mut LIMIT_FIELD_BASIS: bool = true;
/// preview the resulting field before committing to a move, requiring explicit confirmation
pub static mut CONFIRM_BEFORE_PLAY: bool = false;
/// show a live log of which hand card was played on which field slot -- purely
/// a display preference (see game/move_log.rs), not synced via GameSettings
/// since it doesn't affect game state or determinism between online peers
pub static mut SHOW_MOVE_LOG: bool = true;

/// Game settings structure for online play
#[derive(Debug, Clone, Copy)]
pub struct GameSettings {
    pub allow_linear_dependence: bool,
    pub allow_limits_beyond_bounds: u8,
    pub inverse_trig_principal_value: u8,
    pub full_compute: bool,
    pub display_ln_for_log: bool,
    pub use_fractional_exponents: bool,
    pub limit_field_basis: bool,
    pub confirm_before_play: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        GameSettings {
            allow_linear_dependence: false,
            allow_limits_beyond_bounds: 1,
            inverse_trig_principal_value: 0,
            full_compute: false,
            display_ln_for_log: false,
            use_fractional_exponents: true,
            limit_field_basis: true,
            confirm_before_play: false,
        }
    }
}

/// Apply settings from online game
pub fn apply_settings(settings: &GameSettings) {
    unsafe {
        ALLOW_LINEAR_DEPENDENCE = settings.allow_linear_dependence;
        ALLOW_LIMITS_BEYOND_BOUNDS = settings.allow_limits_beyond_bounds;
        INVERSE_TRIG_PRINCIPAL_VALUE = settings.inverse_trig_principal_value;
        FULL_COMPUTE = settings.full_compute;
        DISPLAY_LN_FOR_LOG = settings.display_ln_for_log;
        USE_FRACTIONAL_EXPONENTS = settings.use_fractional_exponents;
        LIMIT_FIELD_BASIS = settings.limit_field_basis;
        CONFIRM_BEFORE_PLAY = settings.confirm_before_play;
    }
}
