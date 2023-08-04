#!/bin/bash
set -e

networks=(kusama rococo polkadot)

rm -rf ~/.cargo/registry/

for network in ${networks[@]}
do
 printf "🏗️ Build "$network" will starting now... \n"
 cargo b -r --features "$network"
 cargo test --features -r "$network"
 wasm_out=./sora2-parachain-runtime_$network.compact.wasm
 mv ./target/release/wbuild/sora2-parachain-runtime_$network.compact.wasm $wasm_out
 if [ -f "$wasm_out" ]; then
    printf "✅ "$wasm_out" OK\n"
 else
    exit 1
 fi
done
