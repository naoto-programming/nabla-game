const path = require('path');
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
				type: 'webassembly/sync',
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
		syncWebAssembly: true,
	},
	devServer: {
		static: {
			directory: dist,
		},
	},
	// the wasm crate is built separately by `yarn build:wasm` (see package.json) into pkg/,
	// since wasm-pack (previously used here via WasmPackPlugin) fails against current
	// rustc/cargo with "invalid type: map, expected a string" -- an unresolved wasm-pack bug
	plugins: [new CopyPlugin({ patterns: [path.resolve(__dirname, 'static')] })],
};
