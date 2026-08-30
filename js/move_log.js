// field/record separators for the `changes` string built in move_log.rs's
// record_move -- non-printable control characters (\x1e/\x1f) chosen there
// specifically so they can't collide with a card's Display text
const CHANGE_RECORD_SEP = '\x1e';
const CHANGE_FIELD_SEP = '\x1f';

// small, self-contained wording table for the connector text around the
// language-neutral data Rust sends (card notation, slot numbers, before/
// after basis text) -- kept local rather than imported from js/i18n.js:
// this file is copied by wasm-bindgen into its own snippet instance (see
// js/online.js's header comment for the same gotcha), so importing i18n.js
// here would get a second, independent copy of its module state, frozen at
// whatever language was active when the page first loaded rather than
// tracking later toggles. Reading `document.documentElement.lang` instead
// (set by js/i18n.js's applyLang) works from any module instance, since
// it's live DOM state, not JS-module-local state
const WORDING = {
	en: { empty: 'empty', slot: 'slot', on: 'on', join: ', ' },
	ja: { empty: '空', slot: 'スロット', on: 'に', join: '、' },
};

// removes the oldest entries (top of the DOM order -- new ones are always
// appended at the bottom) until the remaining ones fit within the panel's
// max-height without scrolling. Always leaves at least the newest entry,
// even if it alone is taller than the panel
const trimToFit = panel => {
	while (panel.children.length > 1 && panel.scrollHeight > panel.clientHeight + 1) {
		panel.removeChild(panel.firstElementChild);
	}
};

/**
 * Appends one line to the on-screen move log panel (see #move-log in
 * static/index.html). The panel itself is always in the DOM; the
 * SHOW_MOVE_LOG setting toggles its `hidden` attribute (see menu.rs) and
 * gates whether move_log.rs ever calls this in the first place, so no
 * visibility check is needed here.
 * @param {Number} playerNum - 1 or 2
 * @param {String} cardsText - the played hand card(s)' notation, already
 *   joined with " + " if more than one (language-neutral, so used as-is)
 * @param {String} changes - one or more "slot|before|after" records
 *   (CHANGE_FIELD_SEP-joined fields, CHANGE_RECORD_SEP-joined records); an
 *   empty before/after field means that slot was/became empty
 * @param {String} borderColour - the mover's configured player colour (see
 *   PLAYER_1_COLOUR/PLAYER_2_COLOUR), applied per-entry since it can differ
 *   turn to turn as the two players alternate
 */
export const js_append_move_log_entry = (playerNum, cardsText, changes, borderColour) => {
	const panel = document.getElementById('move-log-entries');
	if (!panel) return;

	const lang = document.documentElement.lang === 'ja' ? 'ja' : 'en';
	const w = WORDING[lang];

	const changesText = changes
		.split(CHANGE_RECORD_SEP)
		.map(record => {
			const [slot, before, after] = record.split(CHANGE_FIELD_SEP);
			const beforeText = before || w.empty;
			const afterText = after || w.empty;
			return `${w.slot} ${slot} (${beforeText} → ${afterText})`;
		})
		.join(w.join);

	const text =
		lang === 'ja'
			? `P${playerNum}: ${cardsText}を${changesText}${w.on}使用`
			: `P${playerNum}: ${cardsText} ${w.on} ${changesText}`;

	const entry = document.createElement('div');
	entry.className = 'move-log-entry';
	entry.style.borderColor = borderColour;
	entry.textContent = text;
	// entries are truncated by default (see .move-log-entry's CSS) since a
	// long move can easily overflow the panel's width -- click to toggle
	// showing the full text instead. Expanding can itself push the panel
	// into overflow, so trim again afterwards, same as on append
	entry.addEventListener('click', () => {
		entry.classList.toggle('expanded');
		trimToFit(panel);
	});
	panel.appendChild(entry);
	trimToFit(panel);
	panel.scrollTop = panel.scrollHeight;
};

/**
 * Clears the move log panel, called once per match (see move_log::reset).
 */
export const js_clear_move_log = () => {
	const panel = document.getElementById('move-log-entries');
	if (panel) panel.innerHTML = '';
};
