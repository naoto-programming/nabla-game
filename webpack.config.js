const path = require('path');
const webpack = require('webpack');
const CopyPlugin = require('copy-webpack-plugin');

const dist = path.resolve(__dirname, 'dist');

module.exports = {
	entry: {
		index: './js/index.js',
	},
	output: {
		path: dist,
		filename: '[name].js',
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
		// substitutes js/online.js's process.env.METERED_API_KEY reference with the real
		// value at build time (from the METERED_API_KEY GitHub Actions secret in CI, or a
		// local shell env var for local builds), so the key lives in neither this repo's
		// source nor its git history -- it's still visible in the deployed bundle to
		// anyone opening devtools (this is a static site with no backend to hide it
		// behind), but at least isn't sitting in plain sight in GitHub code search
		new webpack.DefinePlugin({
			'process.env.METERED_API_KEY': JSON.stringify(process.env.METERED_API_KEY || ''),
		}),
	],
};
