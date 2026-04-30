build:
    curl -OL https://github.com/bytecodealliance/wasmtime/releases/download/v44.0.0/wasi_snapshot_preview1.reactor.wasm
    wkg wit build --wit-dir wit -o tofuya-interface.wasm
