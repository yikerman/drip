# Drip RAW

The bundled RAW stack: LibRaw 0.22.2, libjpeg-turbo and zlib, built from
`vendor/` without system codec libraries, behind one safe call,
`drip_raw::decode`. It returns the visible Bayer samples and owned capture
metadata, including any lens correction tables the camera embedded. Values
stay as the file stores them: interpretation belongs to Drip's nodes.

## Files

| File | Owns |
|---|---|
| `build.rs`, `build/` | static builds of the vendored sources |
| `shim/shim.c` | the C side of LibRaw: open, copy fields into `drip_raw_info`, close |
| `shim/stream.cpp` | reads LibRaw's C++ stream for the metadata callbacks |
| `src/lib.rs` | `decode`, its errors and the layout `drip_raw_info` shares with Rust |
| `src/metadata.rs` | the owned metadata `decode` returns |
| `src/lens.rs` | embedded lens correction tags, picked and validated as darktable does |
| `tests/decode.rs` | decoding and the correction data of the fixtures |
