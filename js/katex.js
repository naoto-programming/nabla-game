import katex from 'katex';

/**
 * Renders a KaTeX expression to a new element and appends it to the DOM
 * @param {String} str - The KaTeX expression to render
 * @returns DOMElement - The rendered element
 */
export const js_render_katex = str => {
	let element = document.createElement('div');
	element.className = 'katex-item';

	katex.render(str, element, { throwOnError: false, displayMode: true });
	document.getElementById('katex').appendChild(element);

	return element;
};

/**
 * Finds the element with id `id`, creating one if not present and renders a KaTeX expression to it.
 * Skips the actual (expensive) KaTeX layout when `str` matches what's already rendered on the
 * element -- draw() re-runs this for every visible card on every animation frame (eg. while a
 * completely unrelated card is being hovered or dealt), so this avoids relaying out long field
 * expressions dozens of times a second for content that hasn't changed.
 * @param {String} str - The KaTeX expression to render
 * @param {String} id - The id of the element on which to render the expression
 * @returns DOMElement - The rendered element
 */
export const js_render_katex_element = (str, id) => {
	let element = document.getElementById(id);
	if (!element && str.length) {
		element = document.createElement('div');
		element.className += 'katex-item';
		element.id = id;
		document.getElementById('katex').appendChild(element);
	}

	if (element && element.dataset.katexSource === str) {
		return element;
	}

	// nothing to render into (eg. clearing a graveyard slot that was never
	// actually populated this match -- see clear_katex_element in katex.rs,
	// called unconditionally for 3 slots regardless of how many cards ended
	// up there) -- katex.render throws trying to set textContent on a null
	// node, so hand back a detached placeholder instead of crashing. Safe
	// because a caller passing an empty str never uses the return value
	if (!element) {
		return document.createElement('div');
	}

	katex.render(str, element, { throwOnError: false, displayMode: true });
	element.dataset.katexSource = str;
	return element;
};
