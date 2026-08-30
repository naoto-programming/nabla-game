/**
 * Appends one line to the on-screen move log panel (see #move-log in
 * static/index.html). The panel itself is always in the DOM; the
 * SHOW_MOVE_LOG setting toggles its `hidden` attribute (see menu.rs) and
 * gates whether move_log.rs ever calls this in the first place, so no
 * visibility check is needed here.
 * @param {String} text - the log line to append
 */
export const js_append_move_log_entry = text => {
	const panel = document.getElementById('move-log-entries');
	if (!panel) return;

	const entry = document.createElement('div');
	entry.className = 'move-log-entry';
	entry.textContent = text;
	panel.appendChild(entry);
	panel.scrollTop = panel.scrollHeight;
};

/**
 * Clears the move log panel, called once per match (see move_log::reset).
 */
export const js_clear_move_log = () => {
	const panel = document.getElementById('move-log-entries');
	if (panel) panel.innerHTML = '';
};
