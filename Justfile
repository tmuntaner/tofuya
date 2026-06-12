pull:
    curl -OL https://github.com/bytecodealliance/wasmtime/releases/download/v44.0.0/wasi_snapshot_preview1.reactor.wasm

build_interface:
    wkg wit build --wit-dir wit -o tofuya-plugin-interface.wasm

build: build_interface pull
    cargo build

release: build_interface pull
    cargo build --release
