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

// Open Relay Project's free, no-signup TURN fallback (used only when a direct
// P2P connection can't be established, eg. restrictive NATs). No "?transport=tcp"
// query string on any of these -- WebKit's RTCPeerConnection throws "Invalid TURN
// URL query string" on that and aborts iceServers setup entirely (not just that one
// URL), which silently broke every connection attempt on iOS/Safari specifically
// while working fine on Chromium. Plain host:port URLs work identically everywhere.
const TURN_CONFIG = [
	{
		urls: ['turn:openrelay.metered.ca:80', 'turn:openrelay.metered.ca:443'],
		username: 'openrelayproject',
		credential: 'openrelayproject',
	},
];

const CODE_CHARS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // no 0/O/1/I/L

const generateRoomCode = () =>
	Array.from({ length: 6 }, () => CODE_CHARS[Math.floor(Math.random() * CODE_CHARS.length)]).join('');

const CONNECT_TIMEOUT_MS = 30000;

let room = null;
let initAction = null;
let moveAction = null;

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

export const js_create_room = () => {
	const code = generateRoomCode();
	room = joinRoom({ appId: APP_ID, turnConfig: TURN_CONFIG }, code, {
		onJoinError: () => on_connection_error(),
	});
	const clearTimeoutFn = withConnectTimeout();
	room.onPeerJoin = () => {
		clearTimeoutFn();
		on_peer_connected();
	};
	room.onPeerLeave = () => on_peer_disconnected();
	attachMessageActions();

	return code;
};

export const js_join_room = code => {
	// room codes are always generated uppercase (see CODE_CHARS above); normalize
	// a manually-typed code so a stray lowercase paste doesn't silently join a
	// different (nonexistent) room and only fail 30s later via the connect timeout
	const normalizedCode = code.trim().toUpperCase();
	room = joinRoom({ appId: APP_ID, turnConfig: TURN_CONFIG }, normalizedCode, {
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

export const js_send_init = (deck, hand1, hand2) => {
	initAction.send({ deck: Array.from(deck), hand1: Array.from(hand1), hand2: Array.from(hand2) });
};

export const js_send_action = clicks => {
	moveAction.send({ clicks });
};

export const js_leave_room = () => {
	if (room) room.leave();
	room = null;
	initAction = null;
	moveAction = null;
};

export const getRoomCodeFromUrl = () => new URLSearchParams(window.location.search).get('room');

export const js_copy_to_clipboard = text => {
	navigator.clipboard && navigator.clipboard.writeText(text).catch(() => {});
};
