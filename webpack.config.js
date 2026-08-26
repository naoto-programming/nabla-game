const path = require('path');
const { execFileSync } = require('child_process');
const webpack = require('webpack');
const CopyPlugin = require('copy-webpack-plugin');

const dist = path.resolve(__dirname, 'dist');

// identifies exactly which build is live (shown bottom-right, see js/index.js) --
// 'unknown' rather than failing the build if git isn't available for some reason
// (eg. a source tarball without a .git directory)
const gitCommitSha = (() => {
	try {
		return execFileSync('git', ['rev-parse', '--short', 'HEAD']).toString().trim();
	} catch {
		return 'unknown';
	}
})();

module.exports = {
	entry: {
		index: './js/index.js',
	},
	output: {
		path: dist,
		// the entry point stays a plain, stable name since index.html hardcodes
		// <script src="index.js"> (this isn't an HtmlWebpackPlugin setup, so nothing
		// rewrites that reference to match a hashed filename)
		filename: '[name].js',
		// but its dynamically split/imported chunks -- which carry essentially all
		// of the actual app code, including online.js -- are NOT referenced by
		// hardcoded HTML; webpack's own runtime (baked into index.js) resolves them
		// by whatever name they're given here, so hashing them is free. Without
		// this, production mode names chunks by plain incrementing IDs (eg.
		// "130.js"), which stay THE SAME across deploys even when their content
		// changes -- combined with GitHub Pages' Cache-Control: max-age=600, a
		// browser could easily keep serving a stale, pre-fix chunk under an
		// unchanged URL for up to 10 minutes (or longer, depending on how
		// aggressively it caches) after a fix actually goes live
		chunkFilename: '[name].[contenthash].js',
	},
	module: {
		rules: [
			{
				test: /\.wasm$/,
				type: 'webassembly/async',
			},
		],
	},
	ignoreWarnings: [
		warning =>
			// temp, see: https://github.com/rust-random/getrandom/issues/224
			warning.message === 'Critical dependency: the request of a dependency is an expression' ||
			warning.message.startsWith('asset size limit:'), // build size warning
	],
	experiments: {
		// async, not sync: webpack's sync wasm parser (@webassemblyjs) can't parse wasm
		// output from current rustc ("parseVec could not cast the value"); async mode
		// just fetches/instantiates the .wasm file via the browser's native WebAssembly
		// APIs at runtime instead of parsing its bytecode into webpack's module graph
		asyncWebAssembly: true,
	},
	devServer: {
		static: {
			directory: dist,
		},
	},
	// the wasm crate is built separately by `yarn build:wasm` (see package.json) into pkg/,
	// since wasm-pack (previously used here via WasmPackPlugin) fails against current
	// rustc/cargo with "invalid type: map, expected a string" -- an unresolved wasm-pack bug
	plugins: [
		new CopyPlugin({ patterns: [path.resolve(__dirname, 'static')] }),
		new webpack.DefinePlugin({
			'process.env.GIT_COMMIT_SHA': JSON.stringify(gitCommitSha),
		}),
	],
};
