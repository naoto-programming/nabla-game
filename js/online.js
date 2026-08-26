import { joinRoom } from '@trystero-p2p/torrent';
// this file is copied by wasm-bindgen into pkg/snippets/<hash>/js/online.js for the
// #[wasm_bindgen(module = "/js/online.js")] extern block in src/game/online.rs to call
// into; importing the exported Rust functions back via this relative path (the
// documented wasm-bindgen JS-snippet convention) is what lets this file call back into
// wasm directly, rather than needing a wasm module reference handed in from elsewhere --
// the naive approach (a separate js/index.js import passing a wasm reference in) silently
// breaks, because wasm-bindgen's copied snippet is a distinct module instance from
// whatever else imports "./online.js" directly, so state set on one is invisible to the other
import {
	on_peer_connected,
	on_peer_disconnected,
	on_connection_error,
	on_init_received,
	on_action_received,
} from '../../../index_bg.js';

// identifies this app in Trystero's public signaling namespace -- not a secret,
// just keeps our rooms from colliding with unrelated apps using the same relays
const APP_ID = 'nabla-game-naoto-programming';

// metered.ca's free-tier TURN fallback (used only when a direct P2P connection
// can't be established, eg. restrictive NATs). Open Relay Project's old static,
// no-signup credentials (username/credential both "openrelayproject") were
// retired -- metered.ca now requires a per-account API key, with the actual
// iceServers list fetched from their REST endpoint rather than hardcoded.
// The key itself is still visible to anyone opening devtools on the deployed
// site (there's no backend to proxy this through), but METERED_API_KEY is
// substituted at build time (see webpack.config.js's DefinePlugin, fed from
// the METERED_API_KEY GitHub Actions secret) rather than committed to source,
// so it isn't sitting in git history / GitHub code search.
const TURN_CREDENTIALS_URL = `https://nabla-game.metered.live/api/v1/turn/credentials?apiKey=${process.env.METERED_API_KEY}`;

// strips any "?transport=..." query string from a TURN/STUN URL. WebKit's
// RTCPeerConnection throws "Invalid TURN URL query string" on ANY iceServers
// entry that has one -- not just that entry, the whole iceServers setup aborts --
// which silently broke every connection attempt on iOS/Safari while working fine
// on Chromium (this bit us once already with the old static config). metered.ca's
// credentials endpoint returns a couple of "?transport=tcp" variants; dropping the
// query string is safe rather than lossy here, since it just falls back to each
// scheme's own default transport (UDP for turn:, TLS/TCP for turns:) which is
// what those variants were asking for anyway.
const stripTransportParam = url => url.split('?')[0];
const sanitizeIceServers = servers =>
	servers.map(server => {
		const urls = Array.isArray(server.urls) ? server.urls : [server.urls];
		const deduped = [...new Set(urls.map(stripTransportParam))];
		return { ...server, urls: deduped.length === 1 ? deduped[0] : deduped };
	});

// fetched once at module load (not per-room) so it's already resolved (or at
// least in flight) by the time the user actually clicks Create/Join a few
// seconds later. Falls back to no TURN servers at all (direct P2P / Trystero's
// own default STUN only) rather than failing room creation outright -- that
// still works for peers without restrictive NATs.
const turnConfigPromise = fetch(TURN_CREDENTIALS_URL)
	.then(res => {
		if (!res.ok) throw new Error(`metered.ca TURN credentials request failed: ${res.status}`);
		return res.json();
	})
	.then(sanitizeIceServers)
	.catch(err => {
		console.warn('Falling back to no TURN servers -- TURN credentials fetch failed:', err);
		return [];
	});

const CODE_CHARS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // no 0/O/1/I/L

const generateRoomCode = () =>
	Array.from({ length: 6 }, () => CODE_CHARS[Math.floor(Math.random() * CODE_CHARS.length)]).join('');

const CONNECT_TIMEOUT_MS = 30000;

let room = null;
let initAction = null;
let moveAction = null;
// invalidates a pending startRoom() call if leave/create/join supersedes it
// before the TURN config fetch resolves (eg. the user cancels or immediately
// creates a new room while the first fetch was still in flight)
let joinToken = 0;

const withConnectTimeout = () => {
	const timer = setTimeout(() => {
		on_connection_error();
	}, CONNECT_TIMEOUT_MS);
	return () => clearTimeout(timer);
};

const attachMessageActions = () => {
	initAction = room.makeAction('init');
	moveAction = room.makeAction('move');

	initAction.onMessage = data => {
		on_init_received(data.deck, data.hand1, data.hand2);
	};
	moveAction.onMessage = data => {
		on_action_received(data.clicks);
	};
};

const startRoom = (code, turnConfig) => {
	room = joinRoom({ appId: APP_ID, turnConfig }, code, {
		onJoinError: () => on_connection_error(),
	});
	const clearTimeoutFn = withConnectTimeout();
	room.onPeerJoin = () => {
		clearTimeoutFn();
		on_peer_connected();
	};
	room.onPeerLeave = () => on_peer_disconnected();
	attachMessageActions();
};

export const js_create_room = () => {
	const code = generateRoomCode();
	const token = ++joinToken;
	turnConfigPromise.then(turnConfig => {
		if (token !== joinToken) return;
		startRoom(code, turnConfig);
	});

	return code;
};

export const js_join_room = code => {
	// room codes are always generated uppercase (see CODE_CHARS above); normalize
	// a manually-typed code so a stray lowercase paste doesn't silently join a
	// different (nonexistent) room and only fail 30s later via the connect timeout
	const normalizedCode = code.trim().toUpperCase();
	const token = ++joinToken;
	turnConfigPromise.then(turnConfig => {
		if (token !== joinToken) return;
		startRoom(normalizedCode, turnConfig);
	});
};

export const js_send_init = (deck, hand1, hand2) => {
	initAction.send({ deck: Array.from(deck), hand1: Array.from(hand1), hand2: Array.from(hand2) });
};

export const js_send_action = clicks => {
	moveAction.send({ clicks });
};

export const js_leave_room = () => {
	joinToken++; // invalidate any join still waiting on the TURN config fetch
	if (room) room.leave();
	room = null;
	initAction = null;
	moveAction = null;
};

export const getRoomCodeFromUrl = () => new URLSearchParams(window.location.search).get('room');

export const js_copy_to_clipboard = text => {
	navigator.clipboard && navigator.clipboard.writeText(text).catch(() => {});
};
