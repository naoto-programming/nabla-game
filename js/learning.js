// this file is copied by wasm-bindgen into pkg/snippets/<hash>/js/learning.js for the
// #[wasm_bindgen(module = "/js/learning.js")] extern block in src/game/learning.rs to
// call into; importing the exported Rust function back via this relative path (the
// documented wasm-bindgen JS-snippet convention) is what lets this file call back into
// wasm directly -- see js/online.js's own header comment for why a wasm reference
// handed in from elsewhere doesn't work here
import { on_learned_data_loaded } from '../../../index_bg.js';

// IndexedDB (not localStorage): the learned table is stored as one compact binary
// blob (see serialize_table in learning.rs) that can grow into the hundreds of KB
// or more with extended self-play -- localStorage is both capped much lower (5-10MB
// depending on browser) and string-only, which would need base64 (a ~33% size
// penalty) to hold binary data at all. A single object store with one fixed-key
// record is enough: this never needs to query by pattern, only load/save the whole
// table at once.
const DB_NAME = 'nabla-learning';
const STORE_NAME = 'state';
const DB_VERSION = 1;
const RECORD_KEY = 'table';

let dbPromise = null;

const openDb = () => {
	if (dbPromise) return dbPromise;
	dbPromise = new Promise((resolve, reject) => {
		const request = indexedDB.open(DB_NAME, DB_VERSION);
		request.onupgradeneeded = () => {
			request.result.createObjectStore(STORE_NAME);
		};
		request.onsuccess = () => resolve(request.result);
		request.onerror = () => reject(request.error);
	});
	return dbPromise;
};

export const js_load_learned_table = () => {
	openDb()
		.then(
			db =>
				new Promise((resolve, reject) => {
					const tx = db.transaction(STORE_NAME, 'readonly');
					const req = tx.objectStore(STORE_NAME).get(RECORD_KEY);
					req.onsuccess = () => resolve(req.result || null);
					req.onerror = () => reject(req.error);
				})
		)
		.then(bytes => {
			on_learned_data_loaded(bytes ? new Uint8Array(bytes) : new Uint8Array());
		})
		.catch(err => {
			// no persisted data yet (first visit), or IndexedDB unavailable/blocked
			// (eg. a private-browsing mode that disables it) -- either way, starting
			// from an empty table is the correct fallback, never a crash
			console.warn('[learning] failed to load persisted table, starting empty:', err);
			on_learned_data_loaded(new Uint8Array());
		});
};

export const js_save_learned_table = bytes => {
	openDb()
		.then(
			db =>
				new Promise((resolve, reject) => {
					const tx = db.transaction(STORE_NAME, 'readwrite');
					tx.objectStore(STORE_NAME).put(bytes, RECORD_KEY);
					tx.oncomplete = () => resolve();
					tx.onerror = () => reject(tx.error);
				})
		)
		.catch(err => {
			console.warn('[learning] failed to persist table:', err);
		});
};
