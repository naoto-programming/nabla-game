const translations = {
	en: {
		'tab.title': 'Nabla Game',
		'menu.main': 'Main Menu',
		'menu.title': 'N𝚫BL𝚫 OPER𝚫TOR G𝚫ME',
		'menu.playvs': 'Play vs Friend',
		'menu.playai': 'Play vs AI',
		'menu.playonline': 'Play Online',
		'online.create': 'Create Game',
		'online.join': 'Join Game',
		'online.roomCode': 'Room Code',
		'online.copyCode': 'Copy Code',
		'online.copyLink': 'Copy Link',
		'online.connect': 'Connect',
		'settings.aiDifficulty': 'AI Difficulty',
		'settings.aiEasy': 'Easy',
		'settings.aiMedium': 'Medium',
		'settings.aiHard': 'Hard',
		'menu.tutorial': 'Instructions',
		'menu.settings': 'Settings',
		'menu.credits': 'Credits',
		'settings.displayLn': 'Display Ln instead of Log ?',
		'settings.linearDependence':
			'Allow linearly dependent field bases ? (ie. bases that are scalar multiples of each other, like x and 2x)',
		'settings.limitsBeyondBounds':
			"Allow limits outside a function's domain ? (ie. arccos/arcsin are only defined for inputs in [-1, 1], so arccos(∞) is normally invalid)",
		'settings.fullCompute': 'Perform all computations ?',
		'settings.fractionalExponents': 'Display roots as x^(1/2) instead of √x',
		'settings.limitFieldBasis': 'Only allow max 3 field basis ?',
		'settings.confirmBeforePlay':
			'Confirm before playing a card ? (shows what the field will look like first)',
		'settings.player1Colour': 'Player 1 Colour',
		'settings.player2Colour': 'Player 2 Colour',
		'settings.cardCounts': 'Change Card Counts',
		'cardCounts.hint':
			'How many of each card are in the deck (includes the 2 copies of 1, x, and x² already dealt onto the starting field)',
		'cardCounts.reset': 'Reset to Default',
		'tutorial.video': 'Japanese Instructional Video:',
		'tutorial.pdf': 'English Translation of PDF Instructions:',
		'credits.builtBy': 'Built by:',
		'credits.githubLink': 'Github Link:',
		'credits.inspiredBy': 'Inspired by:',
		'gameover.title': 'Game Over',
		'gameover.restart': 'Restart?',
		'gameover.copyMatchData': 'Copy Match Data',
		'lang.toggle': '日本語',
	},
	ja: {
		'tab.title': 'ナブラ演算子ゲーム',
		'menu.main': 'メインメニュー',
		'menu.title': 'ナブラ演算子ゲーム',
		'menu.playvs': 'フレンド対戦',
		'menu.playai': 'AI対戦',
		'menu.playonline': 'オンライン対戦',
		'online.create': '対戦を作成',
		'online.join': '対戦に参加',
		'online.roomCode': 'ルームコード',
		'online.copyCode': 'コードをコピー',
		'online.copyLink': 'リンクをコピー',
		'online.connect': '接続',
		'settings.aiDifficulty': 'AIの難易度',
		'settings.aiEasy': '簡単',
		'settings.aiMedium': '普通',
		'settings.aiHard': '難しい',
		'menu.tutorial': '遊び方',
		'menu.settings': '設定',
		'menu.credits': 'クレジット',
		'settings.displayLn': 'log の代わりに ln を表示する？',
		'settings.linearDependence':
			'線型従属な基底をフィールドに置けるようにする？（例：x と 2x のように、互いに定数倍の関係にある基底）',
		'settings.limitsBeyondBounds':
			'定義域外の極限を許可する？（例：arccos・arcsin の定義域は [-1, 1] のみのため、通常 arccos(∞) は無効）',
		'settings.fullCompute': 'すべての計算を実行する？',
		'settings.fractionalExponents': '√x の代わりに x^(1/2) の形で累乗根を表示する',
		'settings.limitFieldBasis': 'フィールドの基底を最大3つまでに制限する？',
		'settings.confirmBeforePlay':
			'カードを使う前に確認する？（先に場がどうなるかを表示します）',
		'settings.player1Colour': 'プレイヤー1の色',
		'settings.player2Colour': 'プレイヤー2の色',
		'settings.cardCounts': 'カードの枚数を変更',
		'cardCounts.hint':
			'各カードの枚数（開始時に場へ配られる 1・x・x² の2枚ずつも含む）',
		'cardCounts.reset': 'デフォルトに戻す',
		'tutorial.video': '日本語の解説動画：',
		'tutorial.pdf': '英語版ルール（PDF翻訳）：',
		'credits.builtBy': '制作：',
		'credits.githubLink': 'GitHubリンク：',
		'credits.inspiredBy': '原作：',
		'gameover.title': 'ゲーム終了',
		'gameover.restart': 'もう一度プレイ？',
		'gameover.copyMatchData': '対局データをコピー',
		'lang.toggle': 'EN',
	},
};

const STORAGE_KEY = 'nabla-lang';

const getInitialLang = () => {
	try {
		const saved = localStorage.getItem(STORAGE_KEY);
		if (saved === 'en' || saved === 'ja') return saved;
	} catch (e) {}
	return navigator.language && navigator.language.startsWith('ja') ? 'ja' : 'en';
};

let currentLang = getInitialLang();

const applyLang = lang => {
	currentLang = lang;
	document.documentElement.lang = lang;
	document.title = translations[lang]['tab.title'];

	document.querySelectorAll('[data-i18n]').forEach(element => {
		const key = element.getAttribute('data-i18n');
		const value = translations[lang][key];
		if (value !== undefined) element.textContent = value;
	});

	const langButton = document.getElementById('button-LANG');
	if (langButton) langButton.textContent = translations[lang]['lang.toggle'];

	try {
		localStorage.setItem(STORAGE_KEY, lang);
	} catch (e) {}
};

// wires up the language toggle button and applies the saved/detected language on load
export const initI18n = () => {
	applyLang(currentLang);

	const langButton = document.getElementById('button-LANG');
	if (langButton) {
		langButton.addEventListener('click', () => applyLang(currentLang === 'en' ? 'ja' : 'en'));
	}
};
