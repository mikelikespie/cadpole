# Third-party notices

Cadpole is Copyright the Cadpole authors and is distributed under the Apache
License, Version 2.0; the full text is in [`LICENSE`](LICENSE). A Cadpole
binary, the `cadpole_wasm` module and the web bundle each contain third-party
code, and this file is the notice that Apache-2.0 section 4(d) and the
notice clauses of MIT, BSD-2-Clause, BSD-3-Clause and ISC require to travel
with them.

**This file is generated.** `bazel run //build_support/licenses:attribution_update`
rewrites it from `build_support/licenses/{crates,npm,fonts,native}.tsv`, and
`bazel test //build_support/licenses:attribution_test` fails when the
committed copy has drifted. Every licence named here has also been checked
against `build_support/licenses/allowed.txt` by
`//build_support/licenses:license_test`, and every version here has been
checked against the RustSec and OSV advisory snapshots by
`//build_support/advisories:advisory_test`.

Where a package publishes no copyright line of its own, this file says so
rather than inventing one; the grant still comes from the licence its own
metadata declares, recorded in the same row.

## Summary

| | Count |
| --- | --- |
| Cadpole's own crates (Apache-2.0) | 22 |
| Third-party crates | 492 |
| npm packages in the web UI | 98 |
| C and C++ libraries from Bazel modules | 4 |
| Bundled fonts | 1 |

Licence terms in force across the third-party code:

| Licence | Packages |
| --- | --- |
| MIT OR Apache-2.0 | 260 |
| MIT | 149 |
| Apache-2.0 | 46 |
| Apache-2.0 OR MIT | 23 |
| Unicode-3.0 | 18 |
| BSD-3-Clause | 14 |
| MIT/Apache-2.0 | 14 |
| ISC | 12 |
| Zlib OR Apache-2.0 OR MIT | 7 |
| Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 5 |
| Apache-2.0/MIT | 5 |
| Unlicense OR MIT | 5 |
| Zlib | 5 |
| BlueOak-1.0.0 | 4 |
| BSL-1.0 | 3 |
| BSD-2-Clause | 2 |
| BSD-2-Clause OR Apache-2.0 OR MIT | 2 |
| BSD-3-Clause OR Apache-2.0 | 2 |
| MIT OR Apache-2.0 OR LGPL-2.1-or-later | 2 |
| MIT OR Zlib OR Apache-2.0 | 2 |
| (Apache-2.0 AND BSD-3-Clause) | 1 |
| (Apache-2.0 OR MIT) AND BSD-3-Clause | 1 |
| (MIT OR Apache-2.0) AND Unicode-3.0 | 1 |
| 0BSD OR MIT OR Apache-2.0 | 1 |
| Apache-2.0 / MIT | 1 |
| Apache-2.0 OR BSL-1.0 | 1 |
| BSD-3-Clause AND MIT | 1 |
| BSD-3-Clause/MIT | 1 |
| CC0-1.0 | 1 |
| CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | 1 |
| CC0-1.0 OR MIT-0 OR Apache-2.0 | 1 |
| MIT AND BSD-3-Clause | 1 |
| MIT OR Apache-2.0 OR Zlib | 1 |
| Unlicense/MIT | 1 |

The policy behind that list -- which terms Cadpole may ship under, and why
MPL, CDDL and every flavour of GPL are refused -- is
`build_support/licenses/allowed.txt`, reproduced at the end of this file.

## Cadpole's own crates

Apache-2.0, Copyright the Cadpole authors.

- `cadpole-assets` 0.1.0
- `cadpole-bench` 0.1.0
- `cadpole-cli` 0.1.0
- `cadpole-core` 0.1.0
- `cadpole-drawing` 0.1.0
- `cadpole-drawing-out` 0.1.0
- `cadpole-engine` 0.1.0
- `cadpole-export-3mf` 0.1.0
- `cadpole-export-step` 0.1.0
- `cadpole-ffi` 0.1.0
- `cadpole-field` 0.1.0
- `cadpole-font` 0.1.0
- `cadpole-import` 0.1.0
- `cadpole-import-2d` 0.1.0
- `cadpole-ops` 0.1.0
- `cadpole-opt` 0.1.0
- `cadpole-print` 0.1.0
- `cadpole-proto` 0.1.0
- `cadpole-render-wgpu` 0.1.0
- `cadpole-script` 0.1.0
- `cadpole-sketch-solve` 0.1.0
- `cadpole-wasm` 0.1.0

## Third-party crates

Every crate in `Cargo.lock`, whether or not a given build configuration
links it: `Cargo.lock` resolves for every platform Cadpole targets, and a
notice file that changed with the host would be useless. 492 crates.

| Package | Version | Licence | Copyright |
| --- | --- | --- | --- |
| `Inflector` | 0.11.4 | BSD-2-Clause | Copyright (c) the Inflector authors: Josh Teeter<joshteeter@gmail.com> |
| `adler2` | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | Copyright (C) Jonas Schievink <jonasschievink@gmail.com> |
| `aho-corasick` | 1.1.5 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| `alloc-no-stdlib` | 2.0.4 | BSD-3-Clause | Copyright (c) 2016 Dropbox, Inc. |
| `alloc-stdlib` | 0.2.4 | BSD-3-Clause | Copyright (c) the alloc-stdlib authors: Daniel Reiter Horn <danielrh@dropbox.com> |
| `allocative` | 0.3.6 | Apache-2.0 | Copyright (c) the allocative authors: Meta |
| `allocative_derive` | 0.3.6 | Apache-2.0 | Copyright (c) the allocative_derive authors: Meta |
| `allocator-api2` | 0.2.21 | MIT OR Apache-2.0 | Copyright (c) the allocator-api2 authors: Zakarum <zaq.dev@icloud.com> |
| `android_system_properties` | 0.1.6 | MIT OR Apache-2.0 | Copyright 2016 Nicolas Silva \| Copyright (c) 2013 Nicolas Silva |
| `annotate-snippets` | 0.9.2 | Apache-2.0/MIT | Copyright 2017 Mozilla |
| `anstream` | 1.0.0 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `anstyle` | 1.0.14 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `anstyle-parse` | 1.0.0 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `anstyle-query` | 1.1.5 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `anstyle-wincon` | 3.0.11 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `anyhow` | 1.0.104 | MIT OR Apache-2.0 | Copyright (c) the anyhow authors: David Tolnay <dtolnay@gmail.com> |
| `approx` | 0.4.0 | Apache-2.0 | Copyright (c) the approx authors: Brendan Zabarauskas <bjzaba@yahoo.com.au> |
| `array-macro` | 2.1.8 | MIT OR Apache-2.0 | Copyright (c) the array-macro authors: Kamila Borowska <kamila@borowska.pw> |
| `arrayref` | 0.3.9 | BSD-2-Clause | Copyright (c) 2015 David Roundy <roundyd@physics.oregonstate.edu> |
| `arrayvec` | 0.7.8 | MIT OR Apache-2.0 | Copyright (c) Ulrik Sverdrup "bluss" 2015-2023 |
| `ash` | 0.38.0+1.3.281 | MIT OR Apache-2.0 | Copyright 2016 Maik Klein \| Copyright (c) 2016 ASH |
| `async-compression` | 0.4.47 | MIT OR Apache-2.0 | Copyright (c) 2018 the rustasync developers |
| `async-trait` | 0.1.92 | MIT OR Apache-2.0 | Copyright (c) the async-trait authors: David Tolnay <dtolnay@gmail.com> |
| `atomic` | 0.5.3 | Apache-2.0/MIT | Copyright (c) 2016 The Rust Project Developers |
| `atomic-polyfill` | 1.0.3 | MIT OR Apache-2.0 | Copyright (c) 2020 Dario Nieuwenhuis |
| `atomic-waker` | 1.1.2 | Apache-2.0 OR MIT | Copyright (c) 2016 Alex Crichton \| Copyright (c) 2017 The Tokio Authors |
| `autocfg` | 1.5.1 | Apache-2.0 OR MIT | Copyright (c) 2018 Josh Stone |
| `axum` | 0.8.9 | MIT | Copyright (c) 2019 axum Contributors |
| `axum-core` | 0.5.6 | MIT | Copyright (c) 2019–2025 axum Contributors |
| `base64` | 0.13.1 | MIT/Apache-2.0 | Copyright (c) 2015 Alice Maz |
| `base64` | 0.22.1 | MIT OR Apache-2.0 | Copyright (c) 2015 Alice Maz |
| `base64` | 0.23.1 | MIT OR Apache-2.0 | Copyright (c) 2025 Alice Maz, Marshall Pierce |
| `beef` | 0.5.2 | MIT OR Apache-2.0 | Copyright 2020 Maciej Hirsz <hello@maciej.codes> \| Copyright (c) 2020 Maciej Hirsz <hello@maciej.codes> |
| `bit-set` | 0.8.0 | Apache-2.0 OR MIT | Copyright (c) 2023 The Rust Project Developers |
| `bit-set` | 0.9.1 | Apache-2.0 OR MIT | Copyright (c) 2026 The Rust Project Developers |
| `bit-vec` | 0.8.0 | Apache-2.0 OR MIT | Copyright (c) 2023 The Rust Project Developers |
| `bit-vec` | 0.9.1 | Apache-2.0 OR MIT | Copyright (c) 2023 The Rust Project Developers |
| `bitflags` | 1.3.2 | MIT/Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `bitflags` | 2.13.2 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `blake3` | 1.8.2 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | Copyright 2019 Jack O'Connor and Samuel Neves |
| `block-buffer` | 0.10.4 | MIT OR Apache-2.0 | Copyright (c) 2018-2019 The RustCrypto Project Developers |
| `block2` | 0.6.2 | MIT | Copyright (c) the block2 authors: Mads Marquart <mads@marquart.dk> |
| `branches` | 0.3.0 | MIT | Copyright (c) 2023 Khashayar Fereidani |
| `brotli` | 8.0.4 | BSD-3-Clause AND MIT | Copyright (c) 2016 Dropbox, Inc. \| Copyright (c) 2009, 2010, 2013-2016 by the Brotli Authors. |
| `brotli-decompressor` | 5.0.3 | BSD-3-Clause/MIT | Copyright (c) 2016 Dropbox, Inc. |
| `bumpalo` | 3.20.3 | MIT OR Apache-2.0 | Copyright (c) 2019 Nick Fitzgerald |
| `bytemuck` | 1.25.2 | Zlib OR Apache-2.0 OR MIT | Copyright (c) 2019 Daniel "Lokathor" Gee. |
| `bytemuck_derive` | 1.12.1 | Zlib OR Apache-2.0 OR MIT | Copyright (c) 2019 Daniel "Lokathor" Gee. |
| `byteorder` | 1.5.0 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| `byteorder-lite` | 0.1.0 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| `bytes` | 1.12.1 | MIT | Copyright (c) 2018 Carl Lerche |
| `cc` | 1.4.6 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `cfg-if` | 1.0.4 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `cfg_aliases` | 0.1.1 | MIT | Copyright (c) 2020 Katharos Technology |
| `cfg_aliases` | 0.2.2 | MIT | Copyright (c) 2020 Katharos Technology |
| `cgmath` | 0.18.0 | Apache-2.0 | Copyright (c) the cgmath authors: Rust game-developers |
| `chrono` | 0.4.45 | MIT OR Apache-2.0 | Copyright (c) 2014, Kang Seonghoon. |
| `clap` | 4.6.6 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `clap_builder` | 4.6.6 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `clap_derive` | 4.6.4 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `clap_lex` | 1.1.0 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `clipboard-win` | 5.4.1 | BSL-1.0 | Copyright (c) the clipboard-win authors: Douman <douman@gmx.se> |
| `clipper2-rust` | 1.1.0 | BSL-1.0 | Copyright (c) the clipper2-rust authors: Lars Brubaker <larsbrubaker@matterhackers.com> |
| `cmp_any` | 0.8.1 | MIT OR Apache-2.0 | Copyright (c) Meta Platforms, Inc. and affiliates. |
| `cobs` | 0.3.0 | MIT OR Apache-2.0 | Copyright (c) 2015 The cobs.rs Developers |
| `codespan-reporting` | 0.13.1 | Apache-2.0 | Copyright (c) the codespan-reporting authors: Brendan Zabarauskas <bjzaba@yahoo.com.au> |
| `colorchoice` | 1.0.5 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `compression-codecs` | 0.4.42 | MIT OR Apache-2.0 | Copyright (c) 2018 the rustasync developers |
| `compression-core` | 0.4.33 | MIT OR Apache-2.0 | Copyright (c) 2018 the rustasync developers |
| `console_error_panic_hook` | 0.1.7 | Apache-2.0/MIT | Copyright (c) 2018 Nick Fitzgerald |
| `constant_time_eq` | 0.3.1 | CC0-1.0 OR MIT-0 OR Apache-2.0 | Copyright (c) the constant_time_eq authors: Cesar Eduardo Barros <cesarb@cesarb.eti.br> |
| `convert_case` | 0.4.0 | MIT | Copyright (c) the convert_case authors: David Purdum <purdum41@gmail.com> |
| `convert_case` | 0.6.0 | MIT | Copyright (c) 2020 David Purdum |
| `cookie` | 0.18.2 | MIT OR Apache-2.0 | Copyright 2017 Sergio Benitez \| Copyright 2014 Alex Chricton \| Copyright (c) 2017 Sergio Benitez \| Copyright (c) 2014 Alex Crichton |
| `cookie_store` | 0.22.1 | MIT OR Apache-2.0 | Copyright (c) 2017 |
| `core-foundation-sys` | 0.8.7 | MIT OR Apache-2.0 | Copyright (c) 2012-2013 Mozilla Foundation |
| `core_detect` | 1.0.0 | MIT/Apache-2.0 | Copyright (c) 2017-2020 The Rust Project Developers |
| `cpufeatures` | 0.2.17 | MIT OR Apache-2.0 | Copyright (c) 2020-2025 The RustCrypto Project Developers |
| `crc` | 3.4.0 | MIT OR Apache-2.0 | Copyright (c) 2017 crc-rs Developers |
| `crc-catalog` | 2.5.0 | MIT OR Apache-2.0 | Copyright (c) the crc-catalog authors: Akhil Velagapudi <akhilvelagapudi@gmail.com> |
| `crc32fast` | 1.5.2 | MIT OR Apache-2.0 | Copyright (c) 2018 Sam Rijs, Alex Crichton and contributors |
| `critical-section` | 1.2.0 | MIT OR Apache-2.0 | Copyright (c) 2022 The critical-section authors |
| `crossbeam-deque` | 0.8.8 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers |
| `crossbeam-epoch` | 0.9.21 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers |
| `crossbeam-utils` | 0.8.23 | MIT OR Apache-2.0 | Copyright (c) 2019 The Crossbeam Project Developers |
| `crunchy` | 0.2.4 | MIT | Copyright 2017-2023 Eira Fransham. |
| `crypto-common` | 0.1.7 | MIT OR Apache-2.0 | Copyright (c) 2021 RustCrypto Developers |
| `ctor` | 1.0.13 | Apache-2.0 OR MIT | Copyright (c) the ctor authors: Matt Mastracci <matthew@mastracci.com> |
| `dashmap` | 6.2.1 | MIT | Copyright (c) 2019 Acrimon |
| `dashu-base` | 0.5.1 | MIT OR Apache-2.0 | Copyright (c) the dashu-base authors: Jacob Zhong <cmpute@gmail.com> |
| `dashu-int` | 0.5.1 | MIT OR Apache-2.0 | Copyright (c) 2020 Tomek Czajka |
| `dashu-ratio` | 0.5.1 | MIT OR Apache-2.0 | Copyright (c) the dashu-ratio authors: Jacob Zhong <cmpute@gmail.com> |
| `data-encoding` | 2.11.1 | MIT | Copyright (c) 2015-2020 Julien Cretin \| Copyright (c) 2017-2020 Google Inc. |
| `debugserver-types` | 0.5.0 | MIT | Copyright (c) the debugserver-types authors: Markus Westerlind <marwes91@gmail.com> |
| `deranged` | 0.5.8 | MIT OR Apache-2.0 | Copyright 2024 Jacob Pratt et al. \| Copyright (c) 2024 Jacob Pratt et al. |
| `derivative` | 2.2.0 | MIT/Apache-2.0 | Copyright (c) 2016 Martin Carton |
| `derive-new` | 0.5.9 | MIT | Copyright (c) 2017-2021 nrc (Nick Cameron) and the derive-new contributors |
| `derive_more` | 0.99.20 | MIT | Copyright (c) 2016 Jelte Fennema |
| `derive_more` | 1.0.0 | MIT | Copyright (c) 2016 Jelte Fennema |
| `derive_more-impl` | 1.0.0 | MIT | Copyright (c) 2016 Jelte Fennema |
| `digest` | 0.10.7 | MIT OR Apache-2.0 | Copyright (c) 2017 Artyom Pavlov |
| `dispatch2` | 0.3.1 | Zlib OR Apache-2.0 OR MIT | Copyright (c) the dispatch2 authors: Mads Marquart <mads@marquart.dk>, Mary <mary@mary.zone> |
| `display_container` | 0.9.0 | MIT OR Apache-2.0 | Copyright (c) the display_container authors: Facebook |
| `displaydoc` | 0.2.7 | MIT OR Apache-2.0 | Copyright (c) the displaydoc authors: Jane Lusby <jlusby@yaah.dev> |
| `document-features` | 0.2.12 | MIT OR Apache-2.0 | Copyright (c) 2020 Olivier Goffart <ogoffart@sixtyfps.io> |
| `dupe` | 0.9.1 | MIT OR Apache-2.0 | Copyright (c) Meta Platforms, Inc. and affiliates. |
| `dupe_derive` | 0.9.1 | MIT OR Apache-2.0 | Copyright (c) Meta Platforms, Inc. and affiliates. |
| `dxf` | 0.6.1 | MIT | Copyright (c) IxMilia. |
| `either` | 1.18.0 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| `embedded-io` | 0.4.0 | MIT OR Apache-2.0 | Copyright (c) 2022 The embedded-io authors |
| `embedded-io` | 0.6.1 | MIT OR Apache-2.0 | Copyright (c) 2023 The embedded-io authors |
| `encoding_rs` | 0.8.41 | (Apache-2.0 OR MIT) AND BSD-3-Clause | Copyright © WHATWG (Apple, Google, Mozilla, Microsoft). |
| `endian-type` | 0.1.2 | MIT | Copyright (c) 2015 |
| `enum_primitive` | 0.1.1 | MIT | Copyright (c) 2015 Anders Kaseorg <andersk@mit.edu> |
| `env_filter` | 2.0.0 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `env_logger` | 0.11.11 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `equivalent` | 1.0.2 | Apache-2.0 OR MIT | Copyright (c) 2016--2023 |
| `erased-serde` | 0.3.31 | MIT OR Apache-2.0 | Copyright (c) the erased-serde authors: David Tolnay <dtolnay@gmail.com> |
| `erased-serde` | 0.4.10 | MIT OR Apache-2.0 | Copyright (c) the erased-serde authors: David Tolnay <dtolnay@gmail.com> |
| `errno` | 0.3.14 | MIT OR Apache-2.0 | Copyright (c) 2014 Chris Wong |
| `error-code` | 3.4.0 | BSL-1.0 | Copyright (c) the error-code authors: Douman <douman@gmx.se> |
| `euclid` | 0.22.14 | MIT OR Apache-2.0 | Copyright (c) 2012-2013 Mozilla Foundation |
| `fancy-regex` | 0.16.2 | MIT | Copyright 2015 The Fancy Regex Authors. |
| `fastrand` | 2.5.0 | Apache-2.0 OR MIT | Copyright (c) the fastrand authors: Stjepan Glavina <stjepang@gmail.com> |
| `fd-lock` | 4.0.4 | MIT OR Apache-2.0 | Copyright 2019 Yoshua Wuyts \| Copyright (c) 2019 Yoshua Wuyts |
| `fdeflate` | 0.3.7 | MIT OR Apache-2.0 | Copyright (c) the fdeflate authors: The image-rs Developers |
| `find-msvc-tools` | 0.1.12 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `fixedbitset` | 0.5.7 | MIT OR Apache-2.0 | Copyright (c) 2015-2017 |
| `flate2` | 1.1.10 | MIT OR Apache-2.0 | Copyright (c) 2014-2026 Alex Crichton |
| `fluent-uri` | 0.1.4 | MIT | Copyright (c) 2021 Scallop Ye |
| `fnv` | 1.0.7 | Apache-2.0 / MIT | Copyright (c) 2017 Contributors |
| `foldhash` | 0.1.5 | Zlib | Copyright (c) 2024 Orson Peters |
| `foldhash` | 0.2.0 | Zlib | Copyright (c) 2024 Orson Peters |
| `font-types` | 0.12.5 | MIT OR Apache-2.0 | Copyright 2019 Fontations Developers \| Copyright (c) 2019 Fontations Developers |
| `fontconfig-parser` | 0.5.8 | MIT | Copyright (c) 2021 Riey |
| `fontdb` | 0.24.0 | MIT | Copyright (c) 2020 Yevhenii Reizner |
| `form_urlencoded` | 1.2.2 | MIT OR Apache-2.0 | Copyright (c) 2013-2016 The rust-url developers |
| `futures-channel` | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton \| Copyright (c) 2017 The Tokio Authors |
| `futures-core` | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton \| Copyright (c) 2017 The Tokio Authors |
| `futures-sink` | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton \| Copyright (c) 2017 The Tokio Authors |
| `futures-task` | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton \| Copyright (c) 2017 The Tokio Authors |
| `futures-util` | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2016 Alex Crichton \| Copyright (c) 2017 The Tokio Authors |
| `fxhash` | 0.2.1 | Apache-2.0/MIT | Copyright (c) the fxhash authors: cbreeden <github@u.breeden.cc> |
| `generic-array` | 0.14.7 | MIT | Copyright (c) 2015 Bartłomiej Kamiński |
| `getrandom` | 0.2.17 | MIT OR Apache-2.0 | Copyright (c) 2018-2024 The rust-random Project Developers \| Copyright (c) 2014 The Rust Project Developers |
| `getrandom` | 0.3.4 | MIT OR Apache-2.0 | Copyright (c) 2018-2025 The rust-random Project Developers \| Copyright (c) 2014 The Rust Project Developers |
| `getrandom` | 0.4.3 | MIT OR Apache-2.0 | Copyright (c) 2018-2026 The rust-random Project Developers \| Copyright (c) 2014 The Rust Project Developers |
| `gpu-allocator` | 0.28.0 | MIT OR Apache-2.0 | Copyright 2021 Traverse Research B.V. \| Copyright (c) 2021 Traverse Research B.V. |
| `gpu-descriptor` | 0.3.2 | MIT OR Apache-2.0 | Copyright (c) the gpu-descriptor authors: Zakarum <zakarumych@ya.ru> |
| `gpu-descriptor-types` | 0.2.0 | MIT OR Apache-2.0 | Copyright (c) the gpu-descriptor-types authors: Zakarum <zakarumych@ya.ru> |
| `half` | 2.7.1 | MIT OR Apache-2.0 | Copyright (c) the half authors: Kathryn Long <squeeself@gmail.com> |
| `harfrust` | 0.13.3 | MIT | Copyright (c) HarfBuzz developers \| Copyright (c) 2020 Yevhenii Reizner |
| `hash32` | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) 2018 Jorge Aparicio |
| `hashbrown` | 0.14.5 | MIT OR Apache-2.0 | Copyright (c) 2016 Amanieu d'Antras |
| `hashbrown` | 0.15.5 | MIT OR Apache-2.0 | Copyright (c) 2016 Amanieu d'Antras |
| `hashbrown` | 0.16.1 | MIT OR Apache-2.0 | Copyright (c) 2016 Amanieu d'Antras |
| `hashbrown` | 0.17.1 | MIT OR Apache-2.0 | Copyright (c) 2016 Amanieu d'Antras |
| `heapless` | 0.7.17 | MIT OR Apache-2.0 | Copyright (c) 2017 Jorge Aparicio |
| `heck` | 0.5.0 | MIT OR Apache-2.0 | Copyright (c) 2015 The Rust Project Developers |
| `hexf-parse` | 0.2.1 | CC0-1.0 | Copyright (c) the hexf-parse authors: Kang Seonghoon <public+rust@mearie.org> |
| `home` | 0.5.12 | MIT OR Apache-2.0 | Copyright (c) the home authors: Brian Anderson <andersrb@gmail.com> |
| `http` | 1.5.0 | MIT OR Apache-2.0 | Copyright 2017 http-rs authors \| Copyright (c) 2017 http-rs authors |
| `http-body` | 1.1.0 | MIT | Copyright (c) 2019-2026 Sean McArthur & Hyper Contributors |
| `http-body-util` | 0.1.5 | MIT | Copyright (c) 2019-2026 Sean McArthur & Hyper Contributors |
| `http-range-header` | 0.4.2 | MIT | Copyright (c) 2021 MarcusGrass |
| `httparse` | 1.10.1 | MIT OR Apache-2.0 | Copyright (c) 2015-2025 Sean McArthur |
| `httpdate` | 1.0.3 | MIT OR Apache-2.0 | Copyright (c) 2016 Pyfisch |
| `hyper` | 1.11.1 | MIT | Copyright (c) 2014-2026 Sean McArthur |
| `hyper-util` | 0.1.20 | MIT | Copyright (c) 2023-2025 Sean McArthur |
| `i_float` | 4.1.0 | MIT | Copyright (c) 2023 iShape-Rust |
| `i_key_sort` | 0.11.0 | MIT | Copyright (c) the i_key_sort authors: Nail Sharipov <nailxsharipov@gmail.com> |
| `i_overlay` | 8.1.1 | MIT OR Apache-2.0 | Copyright (c) the i_overlay authors: Nail Sharipov <nailxsharipov@gmail.com> |
| `i_shape` | 4.0.0 | MIT | Copyright (c) 2023 iShape-Rust |
| `i_tree` | 0.19.0 | MIT | Copyright (c) 2024 iShape-Rust |
| `iana-time-zone` | 0.1.65 | MIT OR Apache-2.0 | Copyright 2020 Andrew Straw \| Copyright (c) 2020 Andrew D. Straw |
| `iana-time-zone-haiku` | 0.1.2 | MIT OR Apache-2.0 | Copyright 2020 Andrew Straw \| Copyright (c) 2020 Andrew D. Straw |
| `icu_collections` | 2.3.0 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `icu_locale_core` | 2.3.0 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `icu_normalizer` | 2.3.0 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `icu_normalizer_data` | 2.3.0 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `icu_properties` | 2.3.0 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `icu_properties_data` | 2.3.0 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `icu_provider` | 2.3.1 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `idna` | 1.1.0 | MIT OR Apache-2.0 | Copyright (c) 2013-2025 The rust-url developers |
| `idna_adapter` | 1.2.2 | Apache-2.0 OR MIT | Copyright (c) The rust-url developers |
| `image` | 0.25.10 | MIT OR Apache-2.0 | Copyright (c) the image authors: The image-rs Developers |
| `indenter` | 0.3.4 | MIT OR Apache-2.0 | Copyright (c) the indenter authors: Jane Lusby <jlusby@yaah.dev> |
| `indexmap` | 2.14.2 | Apache-2.0 OR MIT | Copyright (c) 2016--2017 |
| `inventory` | 0.3.24 | MIT OR Apache-2.0 | Copyright (c) the inventory authors: David Tolnay <dtolnay@gmail.com> |
| `is_terminal_polyfill` | 1.70.2 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `itertools` | 0.10.5 | MIT/Apache-2.0 | Copyright (c) 2015 |
| `itertools` | 0.13.0 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| `itertools` | 0.14.0 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| `itoa` | 1.0.18 | MIT OR Apache-2.0 | Copyright (c) the itoa authors: David Tolnay <dtolnay@gmail.com> |
| `js-sys` | 0.3.98 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `katexit` | 0.1.5 | MIT OR Apache-2.0 | *no notice published by the author* |
| `kurbo` | 0.13.1 | Apache-2.0 OR MIT | Copyright (c) 2018 Raph Levien |
| `lazy_static` | 1.5.0 | MIT OR Apache-2.0 | Copyright (c) 2010 The Rust Project Developers |
| `libc` | 0.2.189 | MIT OR Apache-2.0 | Copyright (c) The Rust Project Developers |
| `libloading` | 0.8.9 | ISC | Copyright © 2015, Simonas Kazlauskas |
| `libm` | 0.2.16 | MIT | Copyright (c) 2018 Jorge Aparicio \| Copyright © 2005-2020 Rich Felker, et al. \| Copyright © 1993,2004 Sun Microsystems or \| Copyright © 2003-2011 David Schultz or |
| `link-section` | 0.19.3 | Apache-2.0 OR MIT | Copyright (c) the link-section authors: Matt Mastracci <matthew@mastracci.com> |
| `linktime-proc-macro` | 0.2.3 | Apache-2.0 OR MIT | Copyright (c) the linktime-proc-macro authors: Matt Mastracci <matthew@mastracci.com> |
| `linux-raw-sys` | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Copyright (c) the linux-raw-sys authors: Dan Gohman <dev@sunfishcode.online> |
| `litemap` | 0.8.3 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `litrs` | 1.0.0 | MIT OR Apache-2.0 | Copyright (c) 2020 Project Developers |
| `lock_api` | 0.4.14 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| `lock_free_hashtable` | 0.1.4 | MIT OR Apache-2.0 | *no notice published by the author* |
| `log` | 0.4.34 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `logos` | 0.15.1 | MIT OR Apache-2.0 | Copyright 2018 Maciej Hirsz <maciej.hirsz@gmail.com> \| Copyright (c) 2018 Maciej Hirsz <maciej.hirsz@gmail.com> |
| `logos-codegen` | 0.15.1 | MIT OR Apache-2.0 | Copyright 2018 Maciej Hirsz <maciej.hirsz@gmail.com> \| Copyright (c) 2018 Maciej Hirsz <maciej.hirsz@gmail.com> |
| `logos-derive` | 0.15.1 | MIT OR Apache-2.0 | Copyright 2018 Maciej Hirsz <maciej.hirsz@gmail.com> \| Copyright (c) 2018 Maciej Hirsz <maciej.hirsz@gmail.com> |
| `lsp-types` | 0.97.0 | MIT | Copyright (c) 2016 Markus Westerlind |
| `lz4_flex` | 0.7.5 | MIT | Copyright (c) the lz4_flex authors: Pascal Seitz <pascal.seitz@gmail.com>, ticki <Ticki@users.noreply.github.com> |
| `lzma-sys` | 0.1.20 | MIT/Apache-2.0 | Copyright (c) 2016 Alex Crichton |
| `manifold-rust` | 0.13.1 | Apache-2.0 | Copyright (c) the manifold-rust authors: Lars Brubaker <larsbrubaker@matterhackers.com> |
| `maplit` | 1.0.2 | MIT/Apache-2.0 | Copyright (c) 2015 |
| `matchers` | 0.2.0 | MIT | Copyright (c) 2019 Eliza Weisman |
| `matchit` | 0.8.4 | MIT AND BSD-3-Clause | Copyright (c) 2022 Ibraheem Ahmed \| Copyright (c) 2013, Julien Schmidt |
| `matext4cgmath` | 0.1.0 | Apache-2.0 | Copyright 2022 Research Institute for Computational Science Co. Ltd |
| `memchr` | 1.0.2 | Unlicense/MIT | Copyright (c) 2015 Andrew Gallant |
| `memchr` | 2.8.3 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| `memoffset` | 0.9.1 | MIT | Copyright (c) 2017 Gilad Naaman |
| `mime` | 0.3.17 | MIT OR Apache-2.0 | Copyright (c) 2014 Sean McArthur |
| `mime_guess` | 2.0.5 | MIT | Copyright (c) 2015 Austin Bonander |
| `minimal-lexical` | 0.2.1 | MIT/Apache-2.0 | Copyright (c) 2009 The Go Authors. All rights reserved. |
| `miniz_oxide` | 0.8.9 | MIT OR Zlib OR Apache-2.0 | Copyright 2013-2014 RAD Game Tools and Valve Software \| Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC \| Copyright (c) 2017 Frommi \| Copyright (c) 2017-2024 oyvindln \| Copyright (c) 2020 Frommi |
| `miniz_oxide` | 0.9.1 | MIT OR Zlib OR Apache-2.0 | Copyright 2013-2014 RAD Game Tools and Valve Software \| Copyright 2010-2014 Rich Geldreich and Tenacious Software LLC \| Copyright (c) 2017 Frommi \| Copyright (c) 2017-2024 oyvindln \| Copyright (c) 2020 Frommi |
| `mio` | 1.2.3 | MIT | Copyright (c) 2014 Carl Lerche and other MIO contributors |
| `moxcms` | 0.8.1 | BSD-3-Clause OR Apache-2.0 | Copyright 2024 Radzivon Bartoshyk \| Copyright (c) Radzivon Bartoshyk. All rights reserved. |
| `multimap` | 0.10.1 | MIT OR Apache-2.0 | Copyright (c) 2016 multimap developers |
| `multiversion` | 0.9.0 | MIT OR Apache-2.0 | Copyright 2019 Caleb Zulawski |
| `multiversion-macros` | 0.9.0 | MIT OR Apache-2.0 | Copyright 2019 Caleb Zulawski |
| `multiversion_no_op` | 1.0.0 | Apache-2.0 OR MIT | Copyright (c) the multiversion_no_op authors: Henri Sivonen <hsivonen@hsivonen.fi> |
| `naga` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `nibble_vec` | 0.1.0 | MIT | Copyright (c) 2015 Michael Sproul |
| `nix` | 0.28.0 | MIT | Copyright (c) 2015 Carl Lerche + nix-rust Authors |
| `nom` | 3.2.1 | MIT | Copyright (c) 2015-2016 Geoffroy Couprie |
| `nom` | 7.1.3 | MIT | Copyright (c) 2014-2019 Geoffroy Couprie |
| `nu-ansi-term` | 0.50.3 | MIT | Copyright (c) 2014 Benjamin Sago \| Copyright (c) 2021-2022 The Nushell Project Developers |
| `num` | 0.4.3 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-bigint` | 0.4.8 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-complex` | 0.4.6 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-conv` | 0.2.2 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt |
| `num-derive` | 0.3.3 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-integer` | 0.1.47 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-iter` | 0.1.46 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-modular` | 0.6.5 | Apache-2.0 | Copyright [2022] [Jacob Zhong] |
| `num-rational` | 0.4.2 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-traits` | 0.1.43 | MIT/Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `num-traits` | 0.2.19 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `objc2` | 0.6.4 | MIT | Copyright (c) the objc2 authors: Mads Marquart <mads@marquart.dk> |
| `objc2-core-foundation` | 0.3.2 | Zlib OR Apache-2.0 OR MIT | *no notice published by the author* |
| `objc2-encode` | 4.1.0 | MIT | Copyright (c) the objc2-encode authors: Mads Marquart <mads@marquart.dk> |
| `objc2-foundation` | 0.3.2 | MIT | *no notice published by the author* |
| `objc2-metal` | 0.3.2 | Zlib OR Apache-2.0 OR MIT | *no notice published by the author* |
| `objc2-quartz-core` | 0.3.2 | Zlib OR Apache-2.0 OR MIT | *no notice published by the author* |
| `once_cell` | 1.21.4 | MIT OR Apache-2.0 | Copyright (c) the once_cell authors: Aleksey Kladov <aleksey.kladov@gmail.com> |
| `once_cell_polyfill` | 1.70.2 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `ordered-float` | 2.10.1 | MIT | Copyright (c) 2015 Jonathan Reem |
| `ordered-float` | 5.5.0 | MIT | Copyright (c) 2015 Jonathan Reem |
| `pagable` | 0.4.2 | Apache-2.0 | *no notice published by the author* |
| `pagable_derive` | 0.4.2 | Apache-2.0 | Copyright (c) the pagable_derive authors: Meta |
| `parking_lot` | 0.12.5 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| `parking_lot_core` | 0.9.12 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| `paste` | 1.0.15 | MIT OR Apache-2.0 | Copyright (c) the paste authors: David Tolnay <dtolnay@gmail.com> |
| `pdf-writer` | 0.15.0 | MIT OR Apache-2.0 | Copyright (c) the pdf-writer authors: Laurenz <laurmaedje@gmail.com>, Martin Haug <mhaug@live.de> |
| `percent-encoding` | 2.3.2 | MIT OR Apache-2.0 | Copyright (c) 2013-2025 The rust-url developers |
| `petgraph` | 0.8.3 | MIT OR Apache-2.0 | Copyright (c) 2015 |
| `pin-project-lite` | 0.2.17 | Apache-2.0 OR MIT | *no notice published by the author* |
| `pkg-config` | 0.3.34 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `png` | 0.18.1 | MIT OR Apache-2.0 | Copyright (c) 2015 nwin |
| `polycool` | 0.4.0 | MIT OR Apache-2.0 | Copyright (c) 2018 Raph Levien |
| `portable-atomic` | 1.15.0 | Apache-2.0 OR MIT | *no notice published by the author* |
| `portable-atomic-util` | 0.2.8 | Apache-2.0 OR MIT | *no notice published by the author* |
| `postcard` | 1.1.3 | MIT OR Apache-2.0 | Copyright (c) 2019 Anthony James Munns |
| `potential_utf` | 0.1.6 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `powerfmt` | 0.2.0 | MIT OR Apache-2.0 | Copyright 2023 Jacob Pratt et al. \| Copyright (c) 2023 Jacob Pratt et al. |
| `ppv-lite86` | 0.2.21 | MIT OR Apache-2.0 | Copyright 2019 The CryptoCorrosion Contributors \| Copyright (c) 2019 The CryptoCorrosion Contributors |
| `presser` | 0.3.1 | MIT OR Apache-2.0 | Copyright (c) 2019 Embark Studios |
| `proc-macro-crate` | 1.3.1 | MIT OR Apache-2.0 | Copyright (c) the proc-macro-crate authors: Bastian Köcher <git@kchr.de> |
| `proc-macro-error` | 1.0.4 | MIT OR Apache-2.0 | Copyright 2019-2020 CreepySkeleton <creepy-skeleton@yandex.ru> \| Copyright (c) 2019-2020 CreepySkeleton |
| `proc-macro-error-attr` | 1.0.4 | MIT OR Apache-2.0 | Copyright 2019-2020 CreepySkeleton <creepy-skeleton@yandex.ru> \| Copyright (c) 2019-2020 CreepySkeleton |
| `proc-macro2` | 1.0.107 | MIT OR Apache-2.0 | Copyright (c) the proc-macro2 authors: David Tolnay <dtolnay@gmail.com>, Alex Crichton <alex@alexcrichton.com> |
| `profiling` | 1.0.18 | MIT OR Apache-2.0 | Copyright (c) the profiling authors: Philip Degarmo <aclysma@gmail.com> |
| `prost` | 0.14.4 | Apache-2.0 | Copyright (c) the prost authors: Dan Burkert <dan@danburkert.com>, Lucio Franco <luciofranco14@gmail.com>, Casper Meijn <casper@meijn.net> |
| `prost-build` | 0.14.4 | Apache-2.0 | Copyright (c) the prost-build authors: Dan Burkert <dan@danburkert.com>, Lucio Franco <luciofranco14@gmail.com>, Casper Meijn <casper@meijn.net> |
| `prost-derive` | 0.14.4 | Apache-2.0 | Copyright (c) the prost-derive authors: Dan Burkert <dan@danburkert.com>, Lucio Franco <luciofranco14@gmail.com>, Casper Meijn <casper@meijn.net> |
| `prost-reflect` | 0.16.5 | MIT OR Apache-2.0 | Copyright (c) the prost-reflect authors: Andrew Hickman <andrew.hickman1@sky.com> |
| `prost-types` | 0.14.4 | Apache-2.0 | Copyright (c) the prost-types authors: Dan Burkert <dan@danburkert.com>, Lucio Franco <luciofranco14@gmail.com>, Casper Meijn <casper@meijn.net> |
| `protoc-gen-prost` | 0.5.0 | Apache-2.0 | Copyright (c) the protoc-gen-prost authors: Marcus Griep <marcus@griep.us>, Sienna Meridian Satterwhite <sienna@r3t.io> |
| `pxfm` | 0.1.30 | BSD-3-Clause OR Apache-2.0 | Copyright 2024 Radzivon Bartoshyk \| Copyright (c) Radzivon Bartoshyk. All rights reserved. |
| `quick-xml` | 0.22.0 | MIT | Copyright (c) 2016 Johann Tuffe |
| `quickcheck` | 1.1.0 | Unlicense OR MIT | Copyright (c) 2015 Andrew Gallant |
| `quote` | 1.0.47 | MIT OR Apache-2.0 | Copyright (c) the quote authors: David Tolnay <dtolnay@gmail.com> |
| `r-efi` | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | *no notice published by the author* |
| `r-efi` | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | *no notice published by the author* |
| `radix_trie` | 0.2.1 | MIT | Copyright (c) 2015 Michael Sproul |
| `rand` | 0.10.2 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `rand` | 0.8.8 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `rand` | 0.9.5 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `rand_chacha` | 0.3.1 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `rand_chacha` | 0.9.0 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `rand_core` | 0.10.1 | MIT OR Apache-2.0 | Copyright (c) 2018-2026 The Rand Project Developers |
| `rand_core` | 0.6.4 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `rand_core` | 0.9.5 | MIT OR Apache-2.0 | Copyright 2018 Developers of the Rand project \| Copyright (c) 2014 The Rust Project Developers |
| `raw-window-handle` | 0.6.2 | MIT OR Apache-2.0 OR Zlib | Copyright (c) 2019 Osspial \| Copyright (c) 2020 Osspial |
| `raw-window-metal` | 1.1.0 | MIT OR Apache-2.0 | *no notice published by the author* |
| `rayon` | 1.12.0 | MIT OR Apache-2.0 | Copyright (c) 2010 The Rust Project Developers |
| `rayon-core` | 1.13.0 | MIT OR Apache-2.0 | Copyright (c) 2010 The Rust Project Developers |
| `rclite` | 0.2.8 | MIT OR Apache-2.0 | Copyright 2023 Khashayar Fereidani \| Copyright (c) 2023 Khashayar Fereidani |
| `read-fonts` | 0.43.3 | MIT OR Apache-2.0 | Copyright 2019 Fontations Developers \| Copyright (c) 2019 Fontations Developers |
| `redox_syscall` | 0.5.18 | MIT | Copyright (c) 2017 Redox OS Developers |
| `ref-cast` | 1.0.27 | MIT OR Apache-2.0 | Copyright (c) the ref-cast authors: David Tolnay <dtolnay@gmail.com> |
| `ref-cast-impl` | 1.0.27 | MIT OR Apache-2.0 | Copyright (c) the ref-cast-impl authors: David Tolnay <dtolnay@gmail.com> |
| `regex` | 1.13.1 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `regex-automata` | 0.4.18 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `regex-syntax` | 0.8.11 | MIT OR Apache-2.0 | Copyright (c) 2014 The Rust Project Developers |
| `renderdoc-sys` | 1.1.0 | MIT OR Apache-2.0 | Copyright (c) 2022 Eyal Kalderon |
| `robust` | 1.2.0 | MIT OR Apache-2.0 | Copyright (c) 2017 The Spade Developers \| Copyright (c) 2020 The GeoRust Project Developers |
| `roxmltree` | 0.20.0 | MIT OR Apache-2.0 | Copyright (c) 2018 Yevhenii Reizner |
| `roxmltree` | 0.21.1 | MIT OR Apache-2.0 | Copyright (c) 2018 Yevhenii Reizner |
| `rustc-hash` | 1.1.0 | Apache-2.0/MIT | Copyright (c) the rustc-hash authors: The Rust Project Developers |
| `rustc-hash` | 2.1.3 | Apache-2.0 OR MIT | Copyright (c) the rustc-hash authors: The Rust Project Developers |
| `rustc_version` | 0.4.1 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| `rustix` | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Copyright (c) the rustix authors: Dan Gohman <dev@sunfishcode.online>, Jakub Konka <kubkon@jakubkonka.com> |
| `ruststep` | 0.4.0 | Apache-2.0 | Copyright (c) the ruststep authors: Toshiki Teramura <teramura@ricos.co.jp>, Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `ruststep-derive` | 0.4.0 | Apache-2.0 | Copyright (c) the ruststep-derive authors: Yoshinori Tanimura <tanimura@ricos.co.jp>, Toshiki Teramura <toshiki.teramura@gmail.com> |
| `rustversion` | 1.0.23 | MIT OR Apache-2.0 | Copyright (c) the rustversion authors: David Tolnay <dtolnay@gmail.com> |
| `rustyline` | 14.0.0 | MIT | Copyright (c) 2015 Katsu Kawakami & Rustyline authors |
| `ryu` | 1.0.23 | Apache-2.0 OR BSL-1.0 | Copyright (c) the ryu authors: David Tolnay <dtolnay@gmail.com> |
| `schemafy` | 0.5.2 | MIT | Copyright (c) 2016 Markus Westerlind |
| `schemafy_core` | 0.5.2 | MIT | Copyright (c) the schemafy_core authors: Markus Westerlind <marwes91@gmail.com> |
| `schemafy_lib` | 0.5.2 | MIT | Copyright (c) the schemafy_lib authors: Markus Westerlind <marwes91@gmail.com>, MATILLAT Quentin <qmatillat@gmail.com> |
| `scopeguard` | 1.2.0 | MIT OR Apache-2.0 | Copyright (c) 2016-2019 Ulrik Sverdrup "bluss" and scopeguard developers |
| `semver` | 1.0.28 | MIT OR Apache-2.0 | Copyright (c) the semver authors: David Tolnay <dtolnay@gmail.com> |
| `sequence_trie` | 0.3.6 | MIT/Apache-2.0 | Copyright 2016 Michael Sproul and contributors. \| Copyright (c) 2016 Michael Sproul and contributors. |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | Copyright (c) the serde authors: Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <dtolnay@gmail.com> |
| `serde-value` | 0.7.0 | MIT | Copyright (c) 2016 arcnmx |
| `serde_core` | 1.0.229 | MIT OR Apache-2.0 | Copyright (c) the serde_core authors: Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <dtolnay@gmail.com> |
| `serde_derive` | 1.0.229 | MIT OR Apache-2.0 | Copyright (c) the serde_derive authors: Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <dtolnay@gmail.com> |
| `serde_json` | 1.0.151 | MIT OR Apache-2.0 | Copyright (c) the serde_json authors: Erick Tryzelaar <erick.tryzelaar@gmail.com>, David Tolnay <dtolnay@gmail.com> |
| `serde_path_to_error` | 0.1.20 | MIT OR Apache-2.0 | Copyright (c) the serde_path_to_error authors: David Tolnay <dtolnay@gmail.com> |
| `serde_repr` | 0.1.21 | MIT OR Apache-2.0 | Copyright (c) the serde_repr authors: David Tolnay <dtolnay@gmail.com> |
| `serde_urlencoded` | 0.7.1 | MIT/Apache-2.0 | Copyright (c) 2016 Anthony Ramine |
| `sha1` | 0.10.7 | MIT OR Apache-2.0 | Copyright (c) 2006-2009 Graydon Hoare \| Copyright (c) 2009-2013 Mozilla Foundation \| Copyright (c) 2016 Artyom Pavlov |
| `sharded-slab` | 0.1.7 | MIT | Copyright (c) 2019 Eliza Weisman |
| `shlex` | 2.0.1 | MIT OR Apache-2.0 | Copyright 2015 Nicholas Allegra (comex). \| Copyright (c) 2015 Nicholas Allegra (comex). |
| `signal-hook-registry` | 1.4.8 | MIT OR Apache-2.0 | Copyright (c) 2017 tokio-jsonrpc developers |
| `simd-adler32` | 0.3.10 | MIT | Copyright (c) [2021] [Marvin Countryman] |
| `simdutf8` | 0.1.5 | MIT OR Apache-2.0 | Copyright (c) the simdutf8 authors: Hans Kratz <hans@appfour.com> |
| `siphasher` | 1.0.3 | MIT/Apache-2.0 | Copyright 2012-2016 The Rust Project Developers. \| Copyright 2016-2026 Frank Denis. |
| `skrifa` | 0.46.2 | MIT OR Apache-2.0 | Copyright 2019 Fontations Developers \| Copyright (c) 2019 Fontations Developers |
| `slab` | 0.4.12 | MIT | Copyright (c) 2019 Carl Lerche |
| `slotmap` | 1.1.1 | Zlib | Copyright (c) 2021 Orson Peters <orsonpeters@gmail.com> |
| `smallvec` | 1.16.1 | MIT OR Apache-2.0 | Copyright (c) 2018 The Servo Project Developers |
| `socket2` | 0.6.5 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `sorted_vector_map` | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) the sorted_vector_map authors: Facebook <opensource+crates-sorted-vector-map@fb.com> |
| `spade` | 2.15.1 | MIT OR Apache-2.0 | Copyright (c) 2017 The Spade Developers |
| `spin` | 0.9.9 | MIT | Copyright (c) 2014 Mathijs van de Nes |
| `spirv` | 0.4.0+sdk-1.4.341.0 | Apache-2.0 | Copyright (c) the spirv authors: Lei Zhang <antiagainst@gmail.com> |
| `stable_deref_trait` | 1.2.1 | MIT OR Apache-2.0 | Copyright (c) 2017 Robert Grosse |
| `starlark` | 0.14.2 | Apache-2.0 | Copyright (c) the starlark authors: Damien Martin-Guillerez <dmarting@google.com>, Stepan Koltsov <stepan.koltsov@gmail.com>, Facebook |
| `starlark_derive` | 0.14.2 | Apache-2.0 | Copyright (c) the starlark_derive authors: Facebook |
| `starlark_map` | 0.14.2 | Apache-2.0 | Copyright (c) the starlark_map authors: Facebook |
| `starlark_syntax` | 0.14.2 | Apache-2.0 | Copyright (c) the starlark_syntax authors: Damien Martin-Guillerez <dmarting@google.com>, Facebook |
| `static_assertions` | 1.1.0 | MIT OR Apache-2.0 | Copyright (c) 2017 Nikolai Vazquez |
| `static_interner` | 0.1.3 | MIT OR Apache-2.0 | *no notice published by the author* |
| `strong_hash` | 0.1.0 | MIT OR Apache-2.0 | *no notice published by the author* |
| `strong_hash_derive` | 0.1.0 | MIT OR Apache-2.0 | *no notice published by the author* |
| `strsim` | 0.10.0 | MIT | Copyright (c) 2015 Danny Guo \| Copyright (c) 2016 Titus Wormer <tituswormer@gmail.com> \| Copyright (c) 2018 Akash Kurdekar |
| `strsim` | 0.11.1 | MIT | Copyright (c) 2015 Danny Guo \| Copyright (c) 2016 Titus Wormer <tituswormer@gmail.com> \| Copyright (c) 2018 Akash Kurdekar |
| `subtle` | 2.6.1 | BSD-3-Clause | Copyright (c) 2016-2017 Isis Agora Lovecruft, Henry de Valence. All rights reserved. \| Copyright (c) 2016-2024 Isis Agora Lovecruft. All rights reserved. |
| `svgtypes` | 0.16.1 | Apache-2.0 OR MIT | Copyright (c) 2018 Yevhenii Reizner |
| `syn` | 1.0.109 | MIT OR Apache-2.0 | Copyright (c) the syn authors: David Tolnay <dtolnay@gmail.com> |
| `syn` | 2.0.119 | MIT OR Apache-2.0 | Copyright (c) the syn authors: David Tolnay <dtolnay@gmail.com> |
| `syn` | 3.0.5 | MIT OR Apache-2.0 | Copyright (c) the syn authors: David Tolnay <dtolnay@gmail.com> |
| `sync_wrapper` | 1.0.2 | Apache-2.0 | Copyright (c) the sync_wrapper authors: Actyx AG <developer@actyx.io> |
| `synstructure` | 0.13.2 | MIT | Copyright 2016 Nika Layzell |
| `take_mut` | 0.2.2 | MIT | Copyright (c) 2016 Sgeo |
| `tempfile` | 3.27.0 | MIT OR Apache-2.0 | Copyright (c) 2015 Steven Allen |
| `textwrap` | 0.11.0 | MIT | Copyright (c) 2016 Martin Geisler |
| `thiserror` | 1.0.69 | MIT OR Apache-2.0 | Copyright (c) the thiserror authors: David Tolnay <dtolnay@gmail.com> |
| `thiserror` | 2.0.20 | MIT OR Apache-2.0 | Copyright (c) the thiserror authors: David Tolnay <dtolnay@gmail.com> |
| `thiserror-impl` | 1.0.69 | MIT OR Apache-2.0 | Copyright (c) the thiserror-impl authors: David Tolnay <dtolnay@gmail.com> |
| `thiserror-impl` | 2.0.20 | MIT OR Apache-2.0 | Copyright (c) the thiserror-impl authors: David Tolnay <dtolnay@gmail.com> |
| `thread_local` | 1.1.10 | MIT OR Apache-2.0 | Copyright (c) 2016 The Rust Project Developers |
| `time` | 0.3.55 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt et al. |
| `time-core` | 0.1.9 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt et al. |
| `time-macros` | 0.2.32 | MIT OR Apache-2.0 | Copyright (c) Jacob Pratt et al. |
| `tinystr` | 0.8.4 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `tinyvec` | 1.13.3 | Zlib OR Apache-2.0 OR MIT | Copyright (c) 2019 Daniel "Lokathor" Gee. |
| `tokio` | 1.53.1 | MIT | Copyright (c) Tokio Contributors |
| `tokio-macros` | 2.7.2 | MIT | Copyright (c) 2019 Yoshua Wuyts \| Copyright (c) Tokio Contributors |
| `tokio-stream` | 0.1.19 | MIT | Copyright (c) Tokio Contributors |
| `tokio-tungstenite` | 0.29.0 | MIT | Copyright (c) 2017 Daniel Abramov \| Copyright (c) 2017 Alexey Galakhov |
| `tokio-util` | 0.7.19 | MIT | Copyright (c) Tokio Contributors |
| `toml_datetime` | 0.6.11 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `toml_edit` | 0.19.15 | MIT OR Apache-2.0 | Copyright (c) Individual contributors |
| `tower` | 0.5.3 | MIT | Copyright (c) 2019 Tower Contributors |
| `tower-http` | 0.6.11 | MIT | Copyright (c) 2019-2021 Tower Contributors |
| `tower-layer` | 0.3.3 | MIT | Copyright (c) 2019 Tower Contributors |
| `tower-service` | 0.3.3 | MIT | Copyright (c) 2019 Tower Contributors |
| `tracing` | 0.1.44 | MIT | Copyright (c) 2019 Tokio Contributors |
| `tracing-attributes` | 0.1.31 | MIT | Copyright (c) 2019 Tokio Contributors |
| `tracing-core` | 0.1.36 | MIT | Copyright (c) 2019 Tokio Contributors |
| `tracing-log` | 0.2.0 | MIT | Copyright (c) 2019 Tokio Contributors |
| `tracing-subscriber` | 0.3.23 | MIT | Copyright (c) 2019 Tokio Contributors |
| `triomphe` | 0.1.16 | MIT OR Apache-2.0 | Copyright (c) 2019 Manish Goregaokar |
| `truck-base` | 0.5.0 | Apache-2.0 | Copyright (c) the truck-base authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `truck-derivers` | 0.1.0 | Apache-2.0 | *no notice published by the author* |
| `truck-geometry` | 0.5.0 | Apache-2.0 | Copyright (c) the truck-geometry authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `truck-geotrait` | 0.4.0 | Apache-2.0 | Copyright (c) the truck-geotrait authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `truck-meshalgo` | 0.4.0 | Apache-2.0 | *no notice published by the author* |
| `truck-modeling` | 0.6.0 | Apache-2.0 | Copyright (c) the truck-modeling authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `truck-polymesh` | 0.6.0 | Apache-2.0 | Copyright (c) the truck-polymesh authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `truck-shapeops` | 0.4.0 | Apache-2.0 | *no notice published by the author* |
| `truck-stepio` | 0.3.0 | Apache-2.0 | Copyright (c) the truck-stepio authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `truck-topology` | 0.6.0 | Apache-2.0 | Copyright (c) the truck-topology authors: Yoshinori Tanimura <tanimura@ricos.co.jp> |
| `tungstenite` | 0.29.0 | MIT OR Apache-2.0 | Copyright (c) 2017 Alexey Galakhov \| Copyright (c) 2016 Jason Housley |
| `typeid` | 1.0.3 | MIT OR Apache-2.0 | Copyright (c) the typeid authors: David Tolnay <dtolnay@gmail.com> |
| `typenum` | 1.20.1 | MIT OR Apache-2.0 | Copyright 2014 Paho Lurie-Gregg \| Copyright (c) 2014 Paho Lurie-Gregg |
| `unicase` | 2.9.0 | MIT OR Apache-2.0 | Copyright (c) 2014-2026 Sean McArthur |
| `unicode-ident` | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | Copyright © 1991-2023 Unicode, Inc. |
| `unicode-segmentation` | 1.13.3 | MIT OR Apache-2.0 | Copyright (c) 2015 The Rust Project Developers |
| `unicode-width` | 0.1.14 | MIT OR Apache-2.0 | Copyright (c) 2015 The Rust Project Developers |
| `unicode-xid` | 0.2.6 | MIT OR Apache-2.0 | Copyright (c) 2015 The Rust Project Developers |
| `ureq` | 3.4.2 | MIT OR Apache-2.0 | Copyright (c) 2019 Martin Algesten |
| `ureq-proto` | 0.6.3 | MIT OR Apache-2.0 | Copyright 2022 Martin Algesten |
| `url` | 2.5.8 | MIT OR Apache-2.0 | Copyright (c) 2013-2025 The rust-url developers |
| `utf8-zero` | 0.8.1 | MIT OR Apache-2.0 | Copyright (c) the utf8-zero authors: Simon Sapin <simon.sapin@exyr.org>, Martin Algesten <martin@algesten.se> |
| `utf8_iter` | 1.0.4 | Apache-2.0 OR MIT | Copyright (c) the utf8_iter authors: Henri Sivonen <hsivonen@hsivonen.fi> |
| `utf8parse` | 0.2.2 | Apache-2.0 OR MIT | Copyright (c) 2016 Joe Wilm |
| `uuid` | 1.26.1 | Apache-2.0 OR MIT | Copyright (c) 2014 The Rust Project Developers \| Copyright (c) 2018 Ashley Mannix, Christopher Armstrong, Dylan DPC, Hunar Roop Kahlon |
| `valuable` | 0.1.1 | MIT | *no notice published by the author* |
| `version_check` | 0.9.5 | MIT/Apache-2.0 | Copyright (c) 2017-2018 Sergio Benitez |
| `vtkio` | 0.6.3 | MIT OR Apache-2.0 | Copyright (c) 2021 Egor Larionov |
| `wasi` | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Copyright (c) the wasi authors: The Cranelift Project Developers |
| `wasip2` | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | *no notice published by the author* |
| `wasm-bindgen` | 0.2.121 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `wasm-bindgen-futures` | 0.4.71 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `wasm-bindgen-macro` | 0.2.121 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `wasm-bindgen-macro-support` | 0.2.121 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `wasm-bindgen-shared` | 0.2.121 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `web-sys` | 0.3.98 | MIT OR Apache-2.0 | Copyright (c) 2014 Alex Crichton |
| `wgpu` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `wgpu-core` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `wgpu-core-deps-apple` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `wgpu-core-deps-windows-linux-android` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `wgpu-hal` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `wgpu-naga-bridge` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `wgpu-types` | 29.0.4 | MIT OR Apache-2.0 | Copyright (c) 2025 The gfx-rs developers |
| `windows` | 0.62.2 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-collections` | 0.3.2 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-core` | 0.62.2 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-future` | 0.3.2 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-implement` | 0.60.2 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-interface` | 0.59.3 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-link` | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-numerics` | 0.3.1 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-result` | 0.4.1 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-strings` | 0.5.1 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-sys` | 0.52.0 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-sys` | 0.59.0 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-sys` | 0.61.2 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-targets` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows-threading` | 0.2.1 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_aarch64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_aarch64_msvc` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_i686_gnu` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_i686_gnullvm` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_i686_msvc` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_x86_64_gnu` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_x86_64_gnullvm` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `windows_x86_64_msvc` | 0.52.6 | MIT OR Apache-2.0 | Copyright (c) Microsoft Corporation. |
| `winnow` | 0.5.40 | MIT | *no notice published by the author* |
| `wit-bindgen` | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Copyright (c) the wit-bindgen authors: Alex Crichton <alex@alexcrichton.com> |
| `writeable` | 0.6.4 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `xml-rs` | 0.8.29 | MIT | Copyright (c) 2014 Vladimir Matveev |
| `xmltree` | 0.9.0 | MIT | Copyright (c) 2015 Andrew Chin |
| `xz2` | 0.1.7 | MIT/Apache-2.0 | Copyright (c) 2016 Alex Crichton |
| `yoke` | 0.8.3 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `yoke-derive` | 0.8.2 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `zerocopy` | 0.8.57 | BSD-2-Clause OR Apache-2.0 OR MIT | Copyright 2023 The Fuchsia Authors \| Copyright 2019 The Fuchsia Authors. |
| `zerocopy-derive` | 0.8.57 | BSD-2-Clause OR Apache-2.0 OR MIT | Copyright 2023 The Fuchsia Authors \| Copyright 2019 The Fuchsia Authors. |
| `zerofrom` | 0.1.8 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `zerofrom-derive` | 0.1.7 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `zerotrie` | 0.2.5 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `zerovec` | 0.11.8 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `zerovec-derive` | 0.11.6 | Unicode-3.0 | Copyright © 2020-2024 Unicode, Inc. |
| `zlib-rs` | 0.6.7 | Zlib | *no notice published by the author* |
| `zmij` | 1.0.23 | MIT | Copyright (c) the zmij authors: David Tolnay <dtolnay@gmail.com> |

## npm packages

Every package in `pnpm-lock.yaml`. `//src/web:dist` is embedded in the
`cadpole` binary and served by `cadpole serve`, so these ship too.

| Package | Version | Licence | Copyright |
| --- | --- | --- | --- |
| `@bufbuild/protobuf` | 2.15.0 | (Apache-2.0 AND BSD-3-Clause) | *no notice published by the author* |
| `@bufbuild/protoc-gen-es` | 2.15.0 | Apache-2.0 | *no notice published by the author* |
| `@bufbuild/protoplugin` | 2.15.0 | Apache-2.0 | *no notice published by the author* |
| `@dimforge/rapier3d-compat` | 0.12.0 | Apache-2.0 | Copyright 2020 Dimforge EURL |
| `@grpc/grpc-js` | 1.14.4 | Apache-2.0 | Copyright (c) Google Inc. |
| `@grpc/proto-loader` | 0.7.15 | Apache-2.0 | Copyright (c) Google Inc. |
| `@grpc/proto-loader` | 0.8.1 | Apache-2.0 | Copyright (c) Google Inc. |
| `@informalsystems/quint` | 0.32.0 | Apache-2.0 | Copyright (c) Informal Systems |
| `@isaacs/fs-minipass` | 4.0.1 | ISC | Copyright (c) Isaac Z. Schlueter and Contributors |
| `@js-sdsl/ordered-map` | 4.4.2 | MIT | Copyright (c) 2021 Zilong Yao |
| `@octokit/endpoint` | 9.0.6 | MIT | Copyright (c) 2018 Octokit contributors |
| `@octokit/openapi-types` | 24.2.0 | MIT | Copyright 2020 Gregor Martynus |
| `@octokit/request` | 8.4.1 | MIT | Copyright (c) 2018 Octokit contributors |
| `@octokit/request-error` | 5.1.1 | MIT | Copyright (c) 2019 Octokit contributors |
| `@octokit/types` | 13.10.0 | MIT | Copyright (c) Gregor Martynus (https://twitter.com/gr2m) |
| `@playwright/test` | 1.63.0 | Apache-2.0 | Copyright (c) Microsoft Corporation |
| `@protobufjs/aspromise` | 1.1.2 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/base64` | 1.1.2 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/codegen` | 2.0.5 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/eventemitter` | 1.1.1 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/fetch` | 1.1.1 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/float` | 1.0.2 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/path` | 1.1.2 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/pool` | 1.1.0 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@protobufjs/utf8` | 1.1.2 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `@sweet-monads/either` | 3.2.0 | MIT | Copyright (c) JSMonk |
| `@sweet-monads/interfaces` | 3.3.0 | MIT | Copyright (c) 2019-2022 Artem Kobzar |
| `@sweet-monads/maybe` | 3.2.0 | MIT | *no notice published by the author* |
| `@tweenjs/tween.js` | 23.1.3 | MIT | Copyright (c) 2010-2012 Tween.js authors. |
| `@types/line-column` | 1.0.2 | MIT | Copyright (c) Microsoft Corporation. |
| `@types/lodash` | 4.17.25 | MIT | Copyright (c) Microsoft Corporation. |
| `@types/lodash.clonedeep` | 4.5.0 | MIT | Copyright (c) Brian Zengel <https://github.com/bczengel>, Ilya Mochalov <https://github.com/chrootsu>, Stepan Mikhaylyuk <https://github.com/stepancar> |
| `@types/node` | 26.5.1 | MIT | Copyright (c) Microsoft Corporation. |
| `@types/seedrandom` | 3.0.8 | MIT | Copyright (c) Microsoft Corporation. |
| `@types/stats.js` | 0.17.4 | MIT | Copyright (c) Microsoft Corporation. |
| `@types/three` | 0.186.0 | MIT | Copyright (c) Microsoft Corporation. |
| `@types/webxr` | 0.5.24 | MIT | Copyright (c) Microsoft Corporation. |
| `@typescript/vfs` | 1.6.4 | MIT | Copyright (c) Microsoft Corporation |
| `adm-zip` | 0.6.1 | MIT | Copyright (c) 2012 Another-D-Mention Software and other contributors |
| `ansi-regex` | 5.0.1 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `ansi-styles` | 4.3.0 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `antlr4ts` | 0.5.0-alpha.4 | BSD-3-Clause | Copyright (c) 2016 The ANTLR Project |
| `bignumber.js` | 9.3.1 | MIT | Copyright © `<2025>` `Michael Mclaughlin` |
| `chalk` | 4.1.2 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `chownr` | 3.0.0 | BlueOak-1.0.0 | Copyright (c) Isaac Z. Schlueter <i@izs.me> (http://blog.izs.me/) |
| `cli-progress` | 3.12.0 | MIT | Copyright (c) 2015-2022 Andi Dittrich |
| `cliui` | 8.0.1 | ISC | Copyright (c) 2015, Contributors |
| `color-convert` | 2.0.1 | MIT | Copyright (c) 2011-2016 Heather Arthur <fayearthur@gmail.com> |
| `color-name` | 1.1.4 | MIT | Copyright (c) 2015 Dmitry Ivanov |
| `cross-spawn` | 7.0.6 | MIT | Copyright (c) 2018 Made With MOXY Lda <hello@moxy.studio> |
| `debug` | 4.4.3 | MIT | Copyright (c) 2014-2017 TJ Holowaychuk <tj@vision-media.ca> \| Copyright (c) 2018-2021 Josh Junon |
| `deprecation` | 2.3.1 | ISC | Copyright (c) Gregor Martynus and contributors |
| `emoji-regex` | 8.0.0 | MIT | Copyright (c) Mathias Bynens |
| `eol` | 0.9.1 | MIT | Copyright (c) Ryan Van Etten |
| `escalade` | 3.2.0 | MIT | Copyright (c) Luke Edwards <luke.edwards05@gmail.com> (lukeed.com) |
| `fflate` | 0.8.3 | MIT | Copyright (c) 2026 Arjun Barrett |
| `get-caller-file` | 2.0.5 | ISC | Copyright 2018 Stefan Penner |
| `has-flag` | 4.0.0 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `immutable` | 4.3.9 | MIT | Copyright (c) 2014-present, Lee Byron and other contributors. |
| `is-fullwidth-code-point` | 3.0.0 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `isarray` | 1.0.0 | MIT | Copyright (c) Julian Gruber |
| `isexe` | 2.0.0 | ISC | Copyright (c) Isaac Z. Schlueter and Contributors |
| `isobject` | 2.1.0 | MIT | Copyright (c) 2014-2016, Jon Schlinkert. |
| `json-bigint` | 1.0.0 | MIT | Copyright (c) 2013 Andrey Sidorov |
| `line-column` | 1.0.2 | MIT | Copyright (c) 2016 IRIDE Monad <iride.monad@gmail.com> |
| `lodash` | 4.18.1 | MIT | Copyright (c) John-David Dalton <john.david.dalton@gmail.com> |
| `lodash.camelcase` | 4.3.0 | MIT | Copyright (c) John-David Dalton <john.david.dalton@gmail.com> (http://allyoucanleet.com/) |
| `lodash.clonedeep` | 4.5.0 | MIT | Copyright (c) John-David Dalton <john.david.dalton@gmail.com> (http://allyoucanleet.com/) |
| `long` | 5.3.2 | Apache-2.0 | Copyright (c) Daniel Wirtz <dcode@dcode.io> |
| `meshoptimizer` | 1.1.1 | MIT | Copyright (c) 2016-2026 Arseny Kapoulkine |
| `minipass` | 7.1.3 | BlueOak-1.0.0 | Copyright (c) Isaac Z. Schlueter <i@izs.me> (http://blog.izs.me/) |
| `minizlib` | 3.1.0 | MIT | Copyright (c) 2017-2023 Isaac Z. Schlueter and Contributors \| Copyright (c) 2017-2023 Node.js contributors. All rights reserved. \| Copyright (c) 2017-2023 Joyent, Inc. and other Node contributors. All rights reserved. |
| `ms` | 2.1.3 | MIT | Copyright (c) 2020 Vercel, Inc. |
| `once` | 1.4.0 | ISC | Copyright (c) Isaac Z. Schlueter and Contributors |
| `path-key` | 3.1.1 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `playwright` | 1.63.0 | Apache-2.0 | Copyright (c) Microsoft Corporation |
| `playwright-core` | 1.63.0 | Apache-2.0 | Copyright (c) Microsoft Corporation |
| `protobufjs` | 7.6.6 | BSD-3-Clause | Copyright (c) 2016, Daniel Wirtz All rights reserved. |
| `require-directory` | 2.1.1 | MIT | Copyright (c) 2011 Troy Goode <troygoode@gmail.com> |
| `seedrandom` | 3.0.5 | MIT | Copyright (c) David Bau |
| `shebang-command` | 2.0.0 | MIT | Copyright (c) Kevin Mårtensson <kevinmartensson@gmail.com> (github.com/kevva) |
| `shebang-regex` | 3.0.0 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `string-width` | 4.2.3 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `strip-ansi` | 6.0.1 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `supports-color` | 7.2.0 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (sindresorhus.com) |
| `tar` | 7.5.22 | BlueOak-1.0.0 | Copyright (c) Isaac Z. Schlueter |
| `three` | 0.186.0 | MIT | Copyright © 2010-2026 three.js authors |
| `typescript` | 5.4.5 | Apache-2.0 | Copyright (c) Microsoft Corp. |
| `typescript` | 5.9.3 | Apache-2.0 | *no notice published by the author* |
| `undici-types` | 8.9.0 | MIT | Copyright (c) Matteo Collina and Undici contributors |
| `universal-user-agent` | 6.0.1 | ISC | Copyright (c) 2018, Gregor Martynus (https://github.com/gr2m) |
| `which` | 2.0.2 | ISC | Copyright (c) Isaac Z. Schlueter and Contributors |
| `wrap-ansi` | 7.0.0 | MIT | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (https://sindresorhus.com) |
| `wrappy` | 1.0.2 | ISC | Copyright (c) Isaac Z. Schlueter and Contributors |
| `y18n` | 5.0.8 | ISC | Copyright (c) 2015, Contributors |
| `yallist` | 5.0.0 | BlueOak-1.0.0 | Copyright (c) Isaac Z. Schlueter <i@izs.me> (http://blog.izs.me/) |
| `yargs` | 17.7.3 | MIT | Copyright 2010 James Halliday (mail@substack.net); Modified work Copyright 2014 Contributors (ben@npmjs.com) |
| `yargs-parser` | 21.1.1 | ISC | Copyright (c) 2016, Contributors |

## C and C++ libraries

C and C++ code a Bazel module compiles into a Cadpole binary, which no
lockfile lists: `build_support/licenses/native.tsv`, which
`//build_support/licenses:license_test` checks against the module graph
and against what the shipped artifacts link. All of it is HiGHS,
linked into the `cadpole` CLI only -- never the wasm module or the C FFI
library -- and the third-party code inside it that its build compiles.

| Package | Version | Licence | Copyright |
| --- | --- | --- | --- |
| `HiGHS` | 1.15.1 | MIT | Copyright (c) 2026 HiGHS \| Copyright (C) 2016-2019 ERGO-Code (IPX, BASICLU) |
| `HiGHS LP file reader (highs/io/filereaderlp)` | 1.15.1 | MIT | Copyright (c) 2020 Michael Feldmeier |
| `pdqsort (extern/pdqsort)` | b1ef26a | Zlib | Copyright (c) 2021 Orson Peters <orsonpeters@gmail.com> |
| `cuPDLP-C (highs/pdlp/cupdlp)` | 1.15.1 | MIT | Copyright (c) 2023 COPT-Public |

## Toolchain and runtime

Third-party code that the toolchains put into a Cadpole artifact, which no
lockfile lists and the tables above therefore do not cover.

- **Rust standard library (`core`, `alloc`, `std`)** — MIT OR Apache-2.0. Statically linked into `cadpole`, `cadpole_wasm` and the C FFI library by rustc itself. Copyright the Rust Project Developers; the same terms as the `rustc-std-workspace-*` rows in the crate table.
- **`libgcc_s.so.1` (GCC runtime: unwinder, soft-float helpers)** — GPL-3.0-or-later WITH GCC-exception-3.1. Dynamically linked into the Linux `cadpole` binary (`ldd` lists it). The GCC Runtime Library Exception is what makes this permissible in a non-GPL program: it grants the right to propagate the runtime's object code in a work that is not GPL, so no obligation flows to Cadpole. This is the one component here that `allowed.txt` would reject if it were a crate, which is why it is named rather than left implicit.
- **libstdc++ (GCC's C++ standard library)** — GPL-3.0-or-later WITH GCC-exception-3.1. Statically linked into the Linux `cadpole` binary, because HiGHS is C++: the hermetic toolchain links `-l:libstdc++.a`, the host GCC's, and only the parts HiGHS calls end up in the binary. The GCC Runtime Library Exception is what permits this in a non-GPL program: everything in the binary is compiled by Clang or rustc, which are Eligible Compilation Processes, so the exception grants the right to propagate the combined object code under Cadpole's own terms and no obligation flows to Cadpole. Named, like `libgcc_s`, because `allowed.txt` would reject it if it were a crate.
- **libc++ (the macOS system C++ library)** — Apache-2.0 WITH LLVM-exception. Dynamically linked into the macOS `cadpole` binary, because HiGHS is C++: Xcode's toolchain links the system's `/usr/lib/libc++.1.dylib`, which ships with macOS and is not copied into any Cadpole artifact.
- **wasm-bindgen generated glue (`cadpole_wasm.js`, `cadpole_wasm_bg.wasm.d.ts`)** — MIT OR Apache-2.0. Emitted by the `wasm-bindgen` crate (in the crate table) and shipped verbatim in `//src/web:dist`, so the generator's terms travel with it.

Not listed, because they are build- and test-time only and reach no
shipped artifact: the pinned Chromium headless shell
(`//build_support/playwright`), the Quint evaluator
(`//build_support/quint`), the Swift.org Linux toolchain
(`//build_support/swift`), the LLVM toolchain, Node, and esbuild —
esbuild's *output* is Cadpole's own code plus the npm packages already
listed above.

## Bundled fonts

Font *data*, not linked code. The SIL Open Font License requires its text
to ship next to the face, which is what the `licence file` column points
at; `reserved font names` is `none` for every face Cadpole bundles so that
subsetting needs no rename.

| Face | Version | Licence | Reserved font names | File | Licence file |
| --- | --- | --- | --- | --- | --- |
| Inter | 4.1 | OFL-1.1 | none | `assets/fonts/Inter.ttf` | `assets/fonts/Inter-OFL.txt` |

## Licence policy

`build_support/licenses/allowed.txt`, verbatim:

```
# Licence policy for Cadpole (Apache-2.0).
#
# A dependency is allowed when at least one of the licence terms it offers
# appears here. Cadpole ships as Apache-2.0, so every term listed is one we can
# comply with while doing that: permissive, no source-disclosure obligation,
# no per-file copyleft.
#
# Deliberately NOT allowed, even as one option in a disjunction we could
# otherwise satisfy is fine (see `r-efi`, which offers MIT OR Apache-2.0 OR
# LGPL and is therefore allowed): GPL, AGPL, LGPL, MPL, CDDL, EPL, SSPL,
# CC-BY-SA as the *only* term. MPL-2.0 is file-level copyleft and was how
# webpki-roots slipped in through ureq's default TLS stack; that is why this
# file exists.
Apache-2.0
MIT
MIT-0
BSD-2-Clause
BSD-3-Clause
ISC
Zlib
0BSD
Unlicense
CC0-1.0
BSL-1.0
Apache-2.0 WITH LLVM-exception
# The Unicode licence (v3 and the older DFS variant) is permissive and
# OSI-approved; it reaches us through the ICU crates behind `idna`.
Unicode-3.0
Unicode-DFS-2016
# The Blue Oak Model License 1.0.0 is OSI-approved and permissive: a copyright
# and patent grant with no source-disclosure obligation and no notice
# requirement at all, plus a defensive patent termination of the same shape as
# Apache-2.0 section 3. It reaches us through `tar` and its dependencies
# (chownr, minipass, yallist), which the pinned Quint CLI uses to unpack its
# evaluator.
BlueOak-1.0.0

# ---------------------------------------------------------------------------
# Data-only exceptions. A `fonts-only:` term is acceptable for a *font file*
# and for nothing else: `license_test` checks crates.tsv against the terms
# above, which exclude these, and fonts.tsv against both lists together. No
# linked code may ever carry one of these licences.
#
# OFL-1.1 (the SIL Open Font License) is here so Cadpole can bundle a real
# typeface with `text(...)`. The OFL's copyleft-like clause binds the *font*,
# not what is set in it or what links the renderer: the FAQ is explicit that
# a font may be bundled with software under any licence (Q1.3), that
# embedding a font in a document does not change the document's licence
# (Q1.13), and that embedding in full or as a subset is allowed (Q1.12).
# Subsetting counts as modification (Q2.6) and requires a rename only when
# the font declares Reserved Font Names — which is why fonts.tsv records
# that column and the test insists it be "none". Ship the licence text next
# to every bundled face.
fonts-only: OFL-1.1
```

## Licence texts

The full text of every licence Cadpole may be bound by — the SPDX canonical
text of each term in `allowed.txt`, vendored under
`build_support/licenses/texts/` by `refresh.py --texts` and reproduced
here because MIT, BSD-2-Clause, BSD-3-Clause and ISC each require their
permission notice to accompany a distribution, and Apache-2.0 section
4(a) a copy of the licence. 16 licences.

A licence a dependency merely *offers* as one branch of a disjunction we
do not take is not reproduced: `r-efi` is `MIT OR Apache-2.0 OR
LGPL-2.1-or-later`, Cadpole takes one of the first two, and printing the
LGPL here would say something untrue about what Cadpole ships.

### 0BSD

`build_support/licenses/texts/0BSD.txt`

```
Copyright (C) YEAR by AUTHOR EMAIL

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### Apache-2.0

`build_support/licenses/texts/Apache-2.0.txt`

```
Apache License
Version 2.0, January 2004
http://www.apache.org/licenses/

TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

1. Definitions.

"License" shall mean the terms and conditions for use, reproduction, and distribution as defined by Sections 1 through 9 of this document.

"Licensor" shall mean the copyright owner or entity authorized by the copyright owner that is granting the License.

"Legal Entity" shall mean the union of the acting entity and all other entities that control, are controlled by, or are under common control with that entity. For the purposes of this definition, "control" means (i) the power, direct or indirect, to cause the direction or management of such entity, whether by contract or otherwise, or (ii) ownership of fifty percent (50%) or more of the outstanding shares, or (iii) beneficial ownership of such entity.

"You" (or "Your") shall mean an individual or Legal Entity exercising permissions granted by this License.

"Source" form shall mean the preferred form for making modifications, including but not limited to software source code, documentation source, and configuration files.

"Object" form shall mean any form resulting from mechanical transformation or translation of a Source form, including but not limited to compiled object code, generated documentation, and conversions to other media types.

"Work" shall mean the work of authorship, whether in Source or Object form, made available under the License, as indicated by a copyright notice that is included in or attached to the work (an example is provided in the Appendix below).

"Derivative Works" shall mean any work, whether in Source or Object form, that is based on (or derived from) the Work and for which the editorial revisions, annotations, elaborations, or other modifications represent, as a whole, an original work of authorship. For the purposes of this License, Derivative Works shall not include works that remain separable from, or merely link (or bind by name) to the interfaces of, the Work and Derivative Works thereof.

"Contribution" shall mean any work of authorship, including the original version of the Work and any modifications or additions to that Work or Derivative Works thereof, that is intentionally submitted to Licensor for inclusion in the Work by the copyright owner or by an individual or Legal Entity authorized to submit on behalf of the copyright owner. For the purposes of this definition, "submitted" means any form of electronic, verbal, or written communication sent to the Licensor or its representatives, including but not limited to communication on electronic mailing lists, source code control systems, and issue tracking systems that are managed by, or on behalf of, the Licensor for the purpose of discussing and improving the Work, but excluding communication that is conspicuously marked or otherwise designated in writing by the copyright owner as "Not a Contribution."

"Contributor" shall mean Licensor and any individual or Legal Entity on behalf of whom a Contribution has been received by Licensor and subsequently incorporated within the Work.

2. Grant of Copyright License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable copyright license to reproduce, prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the Work and such Derivative Works in Source or Object form.

3. Grant of Patent License. Subject to the terms and conditions of this License, each Contributor hereby grants to You a perpetual, worldwide, non-exclusive, no-charge, royalty-free, irrevocable (except as stated in this section) patent license to make, have made, use, offer to sell, sell, import, and otherwise transfer the Work, where such license applies only to those patent claims licensable by such Contributor that are necessarily infringed by their Contribution(s) alone or by combination of their Contribution(s) with the Work to which such Contribution(s) was submitted. If You institute patent litigation against any entity (including a cross-claim or counterclaim in a lawsuit) alleging that the Work or a Contribution incorporated within the Work constitutes direct or contributory patent infringement, then any patent licenses granted to You under this License for that Work shall terminate as of the date such litigation is filed.

4. Redistribution. You may reproduce and distribute copies of the Work or Derivative Works thereof in any medium, with or without modifications, and in Source or Object form, provided that You meet the following conditions:

     (a) You must give any other recipients of the Work or Derivative Works a copy of this License; and

     (b) You must cause any modified files to carry prominent notices stating that You changed the files; and

     (c) You must retain, in the Source form of any Derivative Works that You distribute, all copyright, patent, trademark, and attribution notices from the Source form of the Work, excluding those notices that do not pertain to any part of the Derivative Works; and

     (d) If the Work includes a "NOTICE" text file as part of its distribution, then any Derivative Works that You distribute must include a readable copy of the attribution notices contained within such NOTICE file, excluding those notices that do not pertain to any part of the Derivative Works, in at least one of the following places: within a NOTICE text file distributed as part of the Derivative Works; within the Source form or documentation, if provided along with the Derivative Works; or, within a display generated by the Derivative Works, if and wherever such third-party notices normally appear. The contents of the NOTICE file are for informational purposes only and do not modify the License. You may add Your own attribution notices within Derivative Works that You distribute, alongside or as an addendum to the NOTICE text from the Work, provided that such additional attribution notices cannot be construed as modifying the License.

     You may add Your own copyright statement to Your modifications and may provide additional or different license terms and conditions for use, reproduction, or distribution of Your modifications, or for any such Derivative Works as a whole, provided Your use, reproduction, and distribution of the Work otherwise complies with the conditions stated in this License.

5. Submission of Contributions. Unless You explicitly state otherwise, any Contribution intentionally submitted for inclusion in the Work by You to the Licensor shall be under the terms and conditions of this License, without any additional terms or conditions. Notwithstanding the above, nothing herein shall supersede or modify the terms of any separate license agreement you may have executed with Licensor regarding such Contributions.

6. Trademarks. This License does not grant permission to use the trade names, trademarks, service marks, or product names of the Licensor, except as required for reasonable and customary use in describing the origin of the Work and reproducing the content of the NOTICE file.

7. Disclaimer of Warranty. Unless required by applicable law or agreed to in writing, Licensor provides the Work (and each Contributor provides its Contributions) on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied, including, without limitation, any warranties or conditions of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE. You are solely responsible for determining the appropriateness of using or redistributing the Work and assume any risks associated with Your exercise of permissions under this License.

8. Limitation of Liability. In no event and under no legal theory, whether in tort (including negligence), contract, or otherwise, unless required by applicable law (such as deliberate and grossly negligent acts) or agreed to in writing, shall any Contributor be liable to You for damages, including any direct, indirect, special, incidental, or consequential damages of any character arising as a result of this License or out of the use or inability to use the Work (including but not limited to damages for loss of goodwill, work stoppage, computer failure or malfunction, or any and all other commercial damages or losses), even if such Contributor has been advised of the possibility of such damages.

9. Accepting Warranty or Additional Liability. While redistributing the Work or Derivative Works thereof, You may choose to offer, and charge a fee for, acceptance of support, warranty, indemnity, or other liability obligations and/or rights consistent with this License. However, in accepting such obligations, You may act only on Your own behalf and on Your sole responsibility, not on behalf of any other Contributor, and only if You agree to indemnify, defend, and hold each Contributor harmless for any liability incurred by, or claims asserted against, such Contributor by reason of your accepting any such warranty or additional liability.

END OF TERMS AND CONDITIONS

APPENDIX: How to apply the Apache License to your work.

To apply the Apache License to your work, attach the following boilerplate notice, with the fields enclosed by brackets "[]" replaced with your own identifying information. (Don't include the brackets!)  The text should be enclosed in the appropriate comment syntax for the file format. We also recommend that a file or class name and description of purpose be included on the same "printed page" as the copyright notice for easier identification within third-party archives.

Copyright [yyyy] [name of copyright owner]

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

### BlueOak-1.0.0

`build_support/licenses/texts/BlueOak-1.0.0.txt`

```
# Blue Oak Model License

Version 1.0.0

## Purpose

This license gives everyone as much permission to work with
this software as possible, while protecting contributors
from liability.

## Acceptance

In order to receive this license, you must agree to its
rules.  The rules of this license are both obligations
under that agreement and conditions to your license.
You must not do anything with this software that triggers
a rule that you cannot or will not follow.

## Copyright

Each contributor licenses you to do everything with this
software that would otherwise infringe that contributor's
copyright in it.

## Notices

You must ensure that everyone who gets a copy of
any part of this software from you, with or without
changes, also gets the text of this license or a link to
<https://blueoakcouncil.org/license/1.0.0>.

## Excuse

If anyone notifies you in writing that you have not
complied with [Notices](#notices), you can keep your
license by taking all practical steps to comply within 30
days after the notice.  If you do not do so, your license
ends immediately.

## Patent

Each contributor licenses you to do everything with this
software that would otherwise infringe any patent claims
they can license or become able to license.

## Reliability

No contributor can revoke this license.

## No Liability

***As far as the law allows, this software comes as is,
without any warranty or condition, and no contributor
will be liable to anyone for any damages related to this
software or this license, under any kind of legal claim.***
```

### BSD-2-Clause

`build_support/licenses/texts/BSD-2-Clause.txt`

```
Copyright (c) <year> <owner> 

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### BSD-3-Clause

`build_support/licenses/texts/BSD-3-Clause.txt`

```
Copyright (c) <year> <owner>. 

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### BSL-1.0

`build_support/licenses/texts/BSL-1.0.txt`

```
Boost Software License - Version 1.0 - August 17th, 2003

Permission is hereby granted, free of charge, to any person or organization obtaining a copy of the software and accompanying documentation covered by this license (the "Software") to use, reproduce, display, distribute, execute, and transmit the Software, and to prepare derivative works of the Software, and to permit third-parties to whom the Software is furnished to do so, all subject to the following:

The copyright notices in the Software and this entire statement, including the above license grant, this restriction and the following disclaimer, must be included in all copies of the Software, in whole or in part, and all derivative works of the Software, unless such copies or derivative works are solely in the form of machine-executable object code generated by a source language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### CC0-1.0

`build_support/licenses/texts/CC0-1.0.txt`

```
Creative Commons Legal Code

CC0 1.0 Universal

    CREATIVE COMMONS CORPORATION IS NOT A LAW FIRM AND DOES NOT PROVIDE
    LEGAL SERVICES. DISTRIBUTION OF THIS DOCUMENT DOES NOT CREATE AN
    ATTORNEY-CLIENT RELATIONSHIP. CREATIVE COMMONS PROVIDES THIS
    INFORMATION ON AN "AS-IS" BASIS. CREATIVE COMMONS MAKES NO WARRANTIES
    REGARDING THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS
    PROVIDED HEREUNDER, AND DISCLAIMS LIABILITY FOR DAMAGES RESULTING FROM
    THE USE OF THIS DOCUMENT OR THE INFORMATION OR WORKS PROVIDED
    HEREUNDER.

Statement of Purpose

The laws of most jurisdictions throughout the world automatically confer
exclusive Copyright and Related Rights (defined below) upon the creator
and subsequent owner(s) (each and all, an "owner") of an original work of
authorship and/or a database (each, a "Work").

Certain owners wish to permanently relinquish those rights to a Work for
the purpose of contributing to a commons of creative, cultural and
scientific works ("Commons") that the public can reliably and without fear
of later claims of infringement build upon, modify, incorporate in other
works, reuse and redistribute as freely as possible in any form whatsoever
and for any purposes, including without limitation commercial purposes.
These owners may contribute to the Commons to promote the ideal of a free
culture and the further production of creative, cultural and scientific
works, or to gain reputation or greater distribution for their Work in
part through the use and efforts of others.

For these and/or other purposes and motivations, and without any
expectation of additional consideration or compensation, the person
associating CC0 with a Work (the "Affirmer"), to the extent that he or she
is an owner of Copyright and Related Rights in the Work, voluntarily
elects to apply CC0 to the Work and publicly distribute the Work under its
terms, with knowledge of his or her Copyright and Related Rights in the
Work and the meaning and intended legal effect of CC0 on those rights.

1. Copyright and Related Rights. A Work made available under CC0 may be
protected by copyright and related or neighboring rights ("Copyright and
Related Rights"). Copyright and Related Rights include, but are not
limited to, the following:

  i. the right to reproduce, adapt, distribute, perform, display,
     communicate, and translate a Work;
 ii. moral rights retained by the original author(s) and/or performer(s);
iii. publicity and privacy rights pertaining to a person's image or
     likeness depicted in a Work;
 iv. rights protecting against unfair competition in regards to a Work,
     subject to the limitations in paragraph 4(a), below;
  v. rights protecting the extraction, dissemination, use and reuse of data
     in a Work;
 vi. database rights (such as those arising under Directive 96/9/EC of the
     European Parliament and of the Council of 11 March 1996 on the legal
     protection of databases, and under any national implementation
     thereof, including any amended or successor version of such
     directive); and
vii. other similar, equivalent or corresponding rights throughout the
     world based on applicable law or treaty, and any national
     implementations thereof.

2. Waiver. To the greatest extent permitted by, but not in contravention
of, applicable law, Affirmer hereby overtly, fully, permanently,
irrevocably and unconditionally waives, abandons, and surrenders all of
Affirmer's Copyright and Related Rights and associated claims and causes
of action, whether now known or unknown (including existing as well as
future claims and causes of action), in the Work (i) in all territories
worldwide, (ii) for the maximum duration provided by applicable law or
treaty (including future time extensions), (iii) in any current or future
medium and for any number of copies, and (iv) for any purpose whatsoever,
including without limitation commercial, advertising or promotional
purposes (the "Waiver"). Affirmer makes the Waiver for the benefit of each
member of the public at large and to the detriment of Affirmer's heirs and
successors, fully intending that such Waiver shall not be subject to
revocation, rescission, cancellation, termination, or any other legal or
equitable action to disrupt the quiet enjoyment of the Work by the public
as contemplated by Affirmer's express Statement of Purpose.

3. Public License Fallback. Should any part of the Waiver for any reason
be judged legally invalid or ineffective under applicable law, then the
Waiver shall be preserved to the maximum extent permitted taking into
account Affirmer's express Statement of Purpose. In addition, to the
extent the Waiver is so judged Affirmer hereby grants to each affected
person a royalty-free, non transferable, non sublicensable, non exclusive,
irrevocable and unconditional license to exercise Affirmer's Copyright and
Related Rights in the Work (i) in all territories worldwide, (ii) for the
maximum duration provided by applicable law or treaty (including future
time extensions), (iii) in any current or future medium and for any number
of copies, and (iv) for any purpose whatsoever, including without
limitation commercial, advertising or promotional purposes (the
"License"). The License shall be deemed effective as of the date CC0 was
applied by Affirmer to the Work. Should any part of the License for any
reason be judged legally invalid or ineffective under applicable law, such
partial invalidity or ineffectiveness shall not invalidate the remainder
of the License, and in such case Affirmer hereby affirms that he or she
will not (i) exercise any of his or her remaining Copyright and Related
Rights in the Work or (ii) assert any associated claims and causes of
action with respect to the Work, in either case contrary to Affirmer's
express Statement of Purpose.

4. Limitations and Disclaimers.

 a. No trademark or patent rights held by Affirmer are waived, abandoned,
    surrendered, licensed or otherwise affected by this document.
 b. Affirmer offers the Work as-is and makes no representations or
    warranties of any kind concerning the Work, express, implied,
    statutory or otherwise, including without limitation warranties of
    title, merchantability, fitness for a particular purpose, non
    infringement, or the absence of latent or other defects, accuracy, or
    the present or absence of errors, whether or not discoverable, all to
    the greatest extent permissible under applicable law.
 c. Affirmer disclaims responsibility for clearing rights of other persons
    that may apply to the Work or any use thereof, including without
    limitation any person's Copyright and Related Rights in the Work.
    Further, Affirmer disclaims responsibility for obtaining any necessary
    consents, permissions or other rights required for any use of the
    Work.
 d. Affirmer understands and acknowledges that Creative Commons is not a
    party to this document and has no duty or obligation with respect to
    this CC0 or use of the Work.
```

### ISC

`build_support/licenses/texts/ISC.txt`

```
ISC License:

Copyright (c) 2004-2010 by Internet Systems Consortium, Inc. ("ISC")
Copyright (c) 1995-2003 by Internet Software Consortium

Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee is hereby granted, provided that the above copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND ISC DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL ISC BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```

### LLVM-exception

`build_support/licenses/texts/LLVM-exception.txt`

```
---- LLVM Exceptions to the Apache 2.0 License ----

   As an exception, if, as a result of your compiling your source code, portions
   of this Software are embedded into an Object form of such source code, you
   may redistribute such embedded portions in such Object form without complying
   with the conditions of Sections 4(a), 4(b) and 4(d) of the License.

   In addition, if you combine or link compiled forms of this Software with
   software that is licensed under the GPLv2 ("Combined Software") and if a
   court of competent jurisdiction determines that the patent provision (Section
   3), the indemnity provision (Section 9) or other Section of the License
   conflicts with the conditions of the GPLv2, you may retroactively and
   prospectively choose to deem waived or otherwise exclude such Section(s) of
   the License, but only in their entirety and only with respect to the Combined
   Software.
```

### MIT

`build_support/licenses/texts/MIT.txt`

```
MIT License

Copyright (c) <year> <copyright holders>

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the
following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial
portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT
LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### MIT-0

`build_support/licenses/texts/MIT-0.txt`

```
MIT No Attribution

Copyright <YEAR> <COPYRIGHT HOLDER>

Permission is hereby granted, free of charge, to any person obtaining a copy of this
software and associated documentation files (the "Software"), to deal in the Software
without restriction, including without limitation the rights to use, copy, modify,
merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### OFL-1.1

`build_support/licenses/texts/OFL-1.1.txt`

```
SIL OPEN FONT LICENSE

Version 1.1 - 26 February 2007

PREAMBLE

The goals of the Open Font License (OFL) are to stimulate worldwide development of collaborative font projects, to support the font creation efforts of academic and linguistic communities, and to provide a free and open framework in which fonts may be shared and improved in partnership with others.

The OFL allows the licensed fonts to be used, studied, modified and redistributed freely as long as they are not sold by themselves. The fonts, including any derivative works, can be bundled, embedded, redistributed and/or sold with any software provided that any reserved names are not used by derivative works. The fonts and derivatives, however, cannot be released under any other type of license. The requirement for fonts to remain under this license does not apply to any document created using the fonts or their derivatives.

DEFINITIONS

"Font Software" refers to the set of files released by the Copyright Holder(s) under this license and clearly marked as such. This may include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the copyright statement(s).

"Original Version" refers to the collection of Font Software components as distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting, or substituting — in part or in whole — any of the components of the Original Version, by changing formats or by porting the Font Software to a new environment.

"Author" refers to any designer, engineer, programmer, technical writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS

Permission is hereby granted, free of charge, to any person obtaining a copy of the Font Software, to use, study, copy, merge, embed, modify, redistribute, and sell modified and unmodified copies of the Font Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components, in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled, redistributed and/or sold with any software, provided that each copy contains the above copyright notice and this license. These can be included either as stand-alone text files, human-readable headers or in the appropriate machine-readable metadata fields within text or binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font Name(s) unless explicit written permission is granted by the corresponding Copyright Holder. This restriction only applies to the primary font name as presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font Software shall not be used to promote, endorse or advertise any Modified Version, except to acknowledge the contribution(s) of the Copyright Holder(s) and the Author(s) or with their explicit written permission.

5) The Font Software, modified or unmodified, in part or in whole, must be distributed entirely under this license, and must not be distributed under any other license. The requirement for fonts to remain under this license does not apply to any document created using the Font Software.

TERMINATION

This license becomes null and void if any of the above conditions are not met.

DISCLAIMER

THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM OTHER DEALINGS IN THE FONT SOFTWARE.
```

### Unicode-3.0

`build_support/licenses/texts/Unicode-3.0.txt`

```
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

### Unicode-DFS-2016

`build_support/licenses/texts/Unicode-DFS-2016.txt`

```
UNICODE, INC. LICENSE AGREEMENT - DATA FILES AND SOFTWARE

Unicode Data Files include all data files under the directories http://www.unicode.org/Public/, http://www.unicode.org/reports/, http://www.unicode.org/cldr/data/, http://source.icu-project.org/repos/icu/, and http://www.unicode.org/utility/trac/browser/.

Unicode Data Files do not include PDF online code charts under the directory http://www.unicode.org/Public/.

Software includes any source code published in the Unicode Standard or under the directories http://www.unicode.org/Public/, http://www.unicode.org/reports/, http://www.unicode.org/cldr/data/, http://source.icu-project.org/repos/icu/, and http://www.unicode.org/utility/trac/browser/.

NOTICE TO USER: Carefully read the following legal agreement. BY DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING UNICODE INC.'S DATA FILES ("DATA FILES"), AND/OR SOFTWARE ("SOFTWARE"), YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2016 Unicode, Inc. All rights reserved. Distributed under the Terms of Use in http://www.unicode.org/copyright.html.

Permission is hereby granted, free of charge, to any person obtaining a copy of the Unicode data files and any associated documentation (the "Data Files") or Unicode software and any associated documentation (the "Software") to deal in the Data Files or Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, and/or sell copies of the Data Files or Software, and to permit persons to whom the Data Files or Software are furnished to do so, provided that either

     (a) this copyright and permission notice appear with all copies of the Data Files or Software, or
     (b) this copyright and permission notice appear in associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF THIRD PARTY RIGHTS. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES, OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall not be used in advertising or otherwise to promote the sale, use or other dealings in these Data Files or Software without prior written authorization of the copyright holder.
```

### Unlicense

`build_support/licenses/texts/Unlicense.txt`

```
This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.

In jurisdictions that recognize copyright laws, the author or authors of this software dedicate any and all copyright interest in the software to the public domain. We make this dedication for the benefit of the public at large and to the detriment of our heirs and
successors. We intend this dedication to be an overt act of relinquishment in perpetuity of all present and future rights to this software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <http://unlicense.org/>
```

### Zlib

`build_support/licenses/texts/Zlib.txt`

```
zlib License

This software is provided 'as-is', without any express or implied warranty.  In no event will the authors be held liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the following restrictions:

     1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.

     2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.

     3. This notice may not be removed or altered from any source distribution.
```

