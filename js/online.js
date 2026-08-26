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
// which say WHY a connection didn't come together (dead TURN server? no ICE
// candidates at all? blocked signaling? one side never got a working
// candidate pair?). Wrapping the native RTCPeerConnection is the standard way
// to see the full picture: every ICE candidate gathered (and its type --
// host/srflx/relay -- which is exactly what says whether a TURN relay
// candidate ever showed up at all), every candidate GATHERING error (fires
// when a STUN/TURN server itself is unreachable or rejects the request), and
// every connection/ICE/signaling state transition, all timestamped.
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
			// address, ie. direct P2P should work), relay (TURN server relay, ie.
			// this candidate came from our TURN config actually being used), prflx
			// (discovered via connectivity checks, rare to see here)
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
			// fires when gathering a candidate from a specific STUN/TURN server
			// fails -- errorCode/errorText come straight from the server (or the
			// browser's attempt to reach it), eg. 401 = bad TURN credentials, 701 =
			// the server itself was unreachable. THIS is usually the single most
			// useful line for diagnosing "TURN doesn't work from this network"
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
// seconds later. Falls back to free STUN/TURN servers when metered.ca is unavailable
// or when METERED_API_KEY is not configured.
const turnConfigPromise = fetch(TURN_CREDENTIALS_URL)
	.then(res => {
		if (!res.ok) throw new Error(`metered.ca TURN credentials request failed: ${res.status}`);
		return res.json();
	})
	.then(sanitizeIceServers)
	.then(servers => {
		console.log(
			'[online] metered.ca TURN credentials fetched successfully:',
			servers.map(s => ({ urls: s.urls, hasCredentials: Boolean(s.username || s.credential) }))
		);
		return servers;
	})
	.catch(err => {
		console.warn('[online] Falling back to free STUN/TURN servers -- TURN credentials fetch failed:', err);
		// Free STUN/TURN servers as fallback for cross-network P2P connections
		return [
			{
				urls: 'stun:stun.l.google.com:19302'
			},
			{
				urls: 'stun:stun1.l.google.com:19302'
			},
			{
				urls: 'stun:stun2.l.google.com:19302'
			},
			{
				urls: 'turn:openrelay.metered.ca:80',
				username: 'openrelayproject',
				credential: 'openrelayproject'
			},
			{
				urls: 'turn:openrelay.metered.ca:443',
				username: 'openrelayproject',
				credential: 'openrelayproject'
			}
		];
	});

const CODE_CHARS = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // no 0/O/1/I/L

const generateRoomCode = () =>
	Array.from({ length: 6 }, () => CODE_CHARS[Math.floor(Math.random() * CODE_CHARS.length)]).join('');

// generous margin for real cross-network conditions -- same-machine testing
// always connects in a few seconds regardless, so it can't validate this number
const CONNECT_TIMEOUT_MS = 45000;

let room = null;
let initAction = null;
let moveAction = null;
// invalidates a pending startRoom() call if leave/create/join supersedes it
// before the TURN config fetch resolves (eg. the user cancels or immediately
// creates a new room while the first fetch was still in flight)
let joinToken = 0;

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

const startRoom = (code, turnConfig) => {
	console.log(`[online] joining room "${code}" (appId="${APP_ID}") with ${turnConfig.length} ICE server entries`);
	const startedAt = Date.now();
	room = joinRoom(
		{
			appId: APP_ID,
			turnConfig,
			// this strategy defaults trickleIce to false (unlike Trystero's other
			// strategies), meaning ICE candidates -- including the TURN server
			// allocation, which itself needs a network round-trip -- must all be
			// fully gathered before any offer/answer is even sent to the peer.
			// Enabling it lets candidates (and the connection) start negotiating as
			// they're found instead of waiting on the slowest one, which matters far
			// more under real internet latency than it ever showed up in same-machine
			// testing (that always connected in a few seconds either way)
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
	console.log(`[online] js_create_room: generated code "${code}", waiting on TURN config...`);
	const token = ++joinToken;
	turnConfigPromise.then(turnConfig => {
		if (token !== joinToken) {
			console.log('[online] js_create_room: superseded before TURN config resolved, not starting room');
			return;
		}
		startRoom(code, turnConfig);
	});

	return code;
};

export const js_join_room = code => {
	// room codes are always generated uppercase (see CODE_CHARS above); normalize
	// a manually-typed code so a stray lowercase paste doesn't silently join a
	// different (nonexistent) room and only fail 30s later via the connect timeout
	const normalizedCode = code.trim().toUpperCase();
	console.log(`[online] js_join_room: code "${normalizedCode}", waiting on TURN config...`);
	const token = ++joinToken;
	turnConfigPromise.then(turnConfig => {
		if (token !== joinToken) {
			console.log('[online] js_join_room: superseded before TURN config resolved, not starting room');
			return;
		}
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
	console.log('[online] js_leave_room');
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
