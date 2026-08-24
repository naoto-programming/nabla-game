![image](https://user-images.githubusercontent.com/10782902/161437902-19001e6b-c7bc-4164-b7b5-2195cbba1497.png)


# Nabla Operator Game ナブラ演算子ゲーム

This project is a web version of the physical card game created by UTokyo students here: https://nablagame.com/.

The source code is written is Rust + WASM, bootstrapped from here: https://github.com/rustwasm/rust-webpack-template.

Additionally, the math engine implements a custom Computer Algebra System (CAS) to calculate arbitrary Derivatives and Integrals, in additional to other algebraic functions such as Mult, Div, Sqrt, Log, etc.

## How to Play

### Play online

- https://nabla-game.netlify.app
- https://naoto-programming.github.io/nabla-game/

### Run it locally

To build and run the game yourself:

1. Install [Rust](https://www.rust-lang.org/tools/install), [Node.js](https://nodejs.org/), and [Yarn](https://yarnpkg.com/).
2. Add the WASM build target: `rustup target add wasm32-unknown-unknown`.
3. Install the matching `wasm-bindgen-cli` (must be the exact same version as the `wasm-bindgen` crate in `Cargo.lock`): `cargo install wasm-bindgen-cli --version <version> --locked`.
4. Install JS dependencies: `yarn install`.
5. Start the dev server: `yarn start` — this builds the WASM binary, launches a local server, and opens the game in your browser.

### Packages used:
- [KaTeX](https://katex.org/) for LaTeX typesetting
- [web-sys](https://rustwasm.github.io/wasm-bindgen/web-sys/index.html) for js DOM structs in Rust
- [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen) for Rust ↔ js ABI communication and build tools
- [gloo](https://github.com/rustwasm/gloo) for better js Event Listener ABIs in Rust


### Future Plans

- Adding WebAudio for browser sounds
- Size optimisation of the final WASM bundle
- Improving responsiveness, currently only mostly supports landscape browsers
- Using WebGL + custom models for the game scene
- Polishing the Menu UI
- Fleshing out the tutorial section
- Eventually improving the custom CAS
- Adding an min-max AI that the user can play against

### Known Issues / Incomplete:

- Integration
  - Integrals don't have full support yet for Complexe Coefficients (log(n), n^(a/b), e^n)
  - Log, Limit operators don't have full support for Integrals
- Inverses
  - Inverses don't support complex integrals
  - Limit operators don't have full support for complex Inverse functions
- Distributive Property (FOIL) is not fully implemented for polynomial x polynomial calculations

### References

- Nabla Operator Game: https://nablagame.com/
- Play Guide: https://www.youtube.com/watch?v=kf0DAygsXAU
- English Rules: https://trans-nabla--itter2voxrtiyag.repl.co/
