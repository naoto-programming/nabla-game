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

// --- verbose WebRTC diagnostics -------------------------------------------
// Trystero only ever surfaces onPeerJoin/onPeerLeave/onJoinError -- none of
// which say WHY a connection didn't come together (no ICE candidates at all?
// blocked signaling? one side never got a working candidate pair?). Wrapping
// the native RTCPeerConnection is the standard way to see the full picture:
// every ICE candidate gathered (and its type -- host/srflx, which is what
// says whether a direct P2P path was actually found), every candidate
// GATHERING error, and every connection/ICE/signaling state transition, all
// timestamped.
//
// To use: open the browser devtools console (F12 or right-click -> Inspect,
// then the "Console" tab) BEFORE creating or joining a room, then attempt the
// connection. Every relevant line is prefixed "[RTC #n]" (n = a per-
// connection counter, since each peer gets its own RTCPeerConnection).
// Copy the full console output (right-click in the console -> "Save as..." in
// Chrome, or select-all + copy) so it can be read back for diagnosis.
if (typeof RTCPeerConnection !== 'undefined') {
	const NativeRTCPeerConnection = RTCPeerConnection;
	let peerCounter = 0;

	const describeIceServers = iceServers =>
		(iceServers || []).map(s => ({
			urls: s.urls,
			hasCredentials: Boolean(s.username || s.credential),
		}));

	window.RTCPeerConnection = function (config, ...rest) {
		const id = ++peerCounter;
		const log = (...args) => console.log(`[RTC #${id}]`, new Date().toISOString(), ...args);

		log('new RTCPeerConnection, iceServers:', describeIceServers(config?.iceServers), 'iceTransportPolicy:', config?.iceTransportPolicy ?? '(default: all)');

		const pc = new NativeRTCPeerConnection(config, ...rest);

		pc.addEventListener('icecandidate', e => {
			if (!e.candidate) {
				log('ICE gathering finished (null candidate signals end-of-candidates)');
				return;
			}
			const c = e.candidate;
			// type: host (direct LAN/local address), srflx (STUN-discovered public
			// address, ie. direct P2P should work), relay (TURN server relay --
			// not used by this app), prflx (discovered via connectivity checks,
			// rare to see here)
			log(
				'local candidate:',
				`type=${c.type}`,
				`protocol=${c.protocol}`,
				`address=${c.address ?? c.ip ?? '(hidden)'}`,
				`port=${c.port}`,
				`relatedAddress=${c.relatedAddress ?? 'n/a'}`
			);
		});
		pc.addEventListener('icecandidateerror', e => {
			// fires when gathering a candidate from a specific STUN server fails --
			// errorCode/errorText come straight from the server (or the browser's
			// attempt to reach it)
			log(
				'ICE CANDIDATE ERROR:',
				`url=${e.url}`,
				`address=${e.address}`,
				`port=${e.port}`,
				`errorCode=${e.errorCode}`,
				`errorText=${e.errorText}`
			);
		});
		pc.addEventListener('iceconnectionstatechange', () => log('iceConnectionState ->', pc.iceConnectionState));
		pc.addEventListener('icegatheringstatechange', () => log('iceGatheringState ->', pc.iceGatheringState));
		pc.addEventListener('connectionstatechange', () => log('connectionState ->', pc.connectionState));
		pc.addEventListener('signalingstatechange', () => log('signalingState ->', pc.signalingState));

		return pc;
	};
	window.RTCPeerConnection.prototype = NativeRTCPeerConnection.prototype;
	window.RTCPeerConnection.generateCertificate = NativeRTCPeerConnection.generateCertificate?.bind(NativeRTCPeerConnection);
}
// --- end verbose WebRTC diagnostics ----------------------------------------

// identifies this app in Trystero's public signaling namespace -- not a secret,
// just keeps our rooms from colliding with unrelated apps using the same relays
const APP_ID = 'nabla-game-naoto-programming';

// Pure P2P: no TURN relay. Trystero's torrent strategy already includes its own
// default STUN servers (Google + Cloudflare, see peer.mjs) even with no
// turnConfig at all, so direct peer-to-peer connections (host/srflx candidates)
// work without any configuration here. A previous version of this file used
// metered.ca's TURN service as a relay fallback for peers behind restrictive
// NATs, but that requires either a paid plan or staying under a small free
// bandwidth quota (500MB/month) shared across every player -- easy to exhaust,
// and once it is, TURN allocate requests just time out silently rather than
// failing loudly, breaking every connection attempt until the quota resets.
// That's worse than having no relay fallback at all. Two peers who can't
// establish a direct path (eg. both behind symmetric NAT) simply won't be
// able to connect to each other -- there's no code-side fix for that without
// paying for or hosting a TURN server.

const CODE_CHARS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // no 0/O/1/I/L

const generateRoomCode = () =>
	Array.from({ length: 6 }, () => CODE_CHARS[Math.floor(Math.random() * CODE_CHARS.length)]).join('');

// generous margin for real cross-network conditions -- same-machine testing
// always connects in a few seconds regardless, so it can't validate this number
const CONNECT_TIMEOUT_MS = 45000;

let room = null;
let initAction = null;
let moveAction = null;

const withConnectTimeout = () => {
	const timer = setTimeout(() => {
		console.warn(`[online] connect timeout fired after ${CONNECT_TIMEOUT_MS}ms with no peer -- giving up`);
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

const startRoom = code => {
	console.log(`[online] joining room "${code}" (appId="${APP_ID}"), P2P only (no TURN)`);
	const startedAt = Date.now();
	room = joinRoom(
		{
			appId: APP_ID,
			// this strategy defaults trickleIce to false (unlike Trystero's other
			// strategies), meaning ICE candidates must all be fully gathered
			// before any offer/answer is even sent to the peer. Enabling it lets
			// candidates (and the connection) start negotiating as they're found
			// instead of waiting on the slowest one
			trickleIce: true,
		},
		code,
		{
			onJoinError: err => {
				console.error('[online] onJoinError -- Trystero could not join the signaling room at all:', err);
				on_connection_error();
			},
		}
	);
	const clearTimeoutFn = withConnectTimeout();
	room.onPeerJoin = peerId => {
		console.log(`[online] onPeerJoin (peer=${peerId}) after ${Date.now() - startedAt}ms -- connection established`);
		clearTimeoutFn();
		on_peer_connected();
	};
	room.onPeerLeave = peerId => {
		console.log(`[online] onPeerLeave (peer=${peerId})`);
		on_peer_disconnected();
	};
	attachMessageActions();
};

export const js_create_room = () => {
	const code = generateRoomCode();
	startRoom(code);
	return code;
};

export const js_join_room = code => {
	// room codes are always generated uppercase (see CODE_CHARS above); normalize
	// a manually-typed code so a stray lowercase paste doesn't silently join a
	// different (nonexistent) room and only fail 30s later via the connect timeout
	const normalizedCode = code.trim().toUpperCase();
	startRoom(normalizedCode);
};

export const js_send_init = (deck, hand1, hand2) => {
	initAction.send({ deck: Array.from(deck), hand1: Array.from(hand1), hand2: Array.from(hand2) });
};

export const js_send_action = clicks => {
	moveAction.send({ clicks });
};

export const js_leave_room = () => {
	console.log('[online] js_leave_room');
	if (room) room.leave();
	room = null;
	initAction = null;
	moveAction = null;
};

export const getRoomCodeFromUrl = () => new URLSearchParams(window.location.search).get('room');

export const js_copy_to_clipboard = text => {
	navigator.clipboard && navigator.clipboard.writeText(text).catch(() => {});
};
