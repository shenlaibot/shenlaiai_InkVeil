# 墨隐 InkVeil 第三方依赖与许可证清单

> 由 `scripts/licenses.py` 自动生成（cargo / npm / python 三个生态，按 SPDX 表达式解析）。
> 策略：GPL / AGPL / LGPL / SSPL / NC 判失败；“未声明/无法解析”判失败；MPL 等弱 copyleft 单独列出，且必须出现在 `THIRD-PARTY-NOTICES.md`。
> Python 部分以 `sidecar/requirements*.lock` 为准；表中“范围”：runtime 随应用分发 / build 与 dev 仅构建期使用 / first-party 本仓库自有。

## 摘要

- 依赖总数：753（cargo 573，npm 136，python 44）
- 不合规：0
- 弱 copyleft（MPL 等）：7

## cargo 依赖（573）

| 包 | 版本 | 许可证 | 范围 |
|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | runtime |
| aead | 0.5.2 | MIT OR Apache-2.0 | runtime |
| aes | 0.8.4 | MIT OR Apache-2.0 | runtime |
| aes-gcm | 0.10.3 | Apache-2.0 OR MIT | runtime |
| aho-corasick | 1.1.5 | Unlicense OR MIT | runtime |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause | runtime |
| alloc-stdlib | 0.2.4 | BSD-3-Clause | runtime |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | runtime |
| anonkit | 0.2.0 | Apache-2.0 | first-party |
| anonymizer-core | 0.2.0 | Apache-2.0 | first-party |
| anonymizer-pipeline | 0.2.0 | Apache-2.0 | first-party |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | runtime |
| arbitrary | 1.4.2 | MIT OR Apache-2.0 | runtime |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 | runtime |
| async-channel | 2.5.0 | Apache-2.0 OR MIT | runtime |
| async-executor | 1.14.0 | Apache-2.0 OR MIT | runtime |
| async-io | 2.6.0 | Apache-2.0 OR MIT | runtime |
| async-lock | 3.4.2 | Apache-2.0 OR MIT | runtime |
| async-process | 2.5.0 | Apache-2.0 OR MIT | runtime |
| async-recursion | 1.1.1 | MIT OR Apache-2.0 | runtime |
| async-signal | 0.2.14 | Apache-2.0 OR MIT | runtime |
| async-task | 4.7.1 | Apache-2.0 OR MIT | runtime |
| async-trait | 0.1.92 | MIT OR Apache-2.0 | runtime |
| atk | 0.18.2 | MIT | runtime |
| atk-sys | 0.18.2 | MIT | runtime |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | runtime |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | runtime |
| base64 | 0.21.7 | MIT OR Apache-2.0 | runtime |
| base64 | 0.22.1 | MIT OR Apache-2.0 | runtime |
| bit-set | 0.8.0 | Apache-2.0 OR MIT | runtime |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT | runtime |
| bitflags | 1.3.2 | MIT OR Apache-2.0 | runtime |
| bitflags | 2.13.1 | MIT OR Apache-2.0 | runtime |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | runtime |
| block2 | 0.6.2 | MIT | runtime |
| blocking | 1.6.2 | Apache-2.0 OR MIT | runtime |
| brotli | 8.0.4 | BSD-3-Clause AND MIT | runtime |
| brotli-decompressor | 5.0.3 | BSD-3-Clause OR MIT | runtime |
| bs58 | 0.5.1 | MIT OR Apache-2.0 | runtime |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | runtime |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| byteorder | 1.5.0 | Unlicense OR MIT | runtime |
| bytes | 1.12.1 | MIT | runtime |
| cairo-rs | 0.18.5 | MIT | runtime |
| cairo-sys-rs | 0.18.2 | MIT | runtime |
| camino | 1.2.5 | MIT OR Apache-2.0 | runtime |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 | runtime |
| cargo_metadata | 0.19.2 | MIT | runtime |
| cargo_toml | 0.22.3 | Apache-2.0 OR MIT | runtime |
| cc | 1.4.2 | MIT OR Apache-2.0 | runtime |
| cesu8 | 1.1.0 | Apache-2.0 OR MIT | runtime |
| cfb | 0.7.3 | MIT | runtime |
| cfg-expr | 0.15.8 | MIT OR Apache-2.0 | runtime |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | runtime |
| cfg_aliases | 0.2.2 | MIT | runtime |
| chacha20 | 0.10.1 | MIT OR Apache-2.0 | runtime |
| chrono | 0.4.45 | MIT OR Apache-2.0 | runtime |
| cipher | 0.4.4 | MIT OR Apache-2.0 | runtime |
| combine | 4.6.7 | MIT | runtime |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | runtime |
| cookie | 0.18.2 | MIT OR Apache-2.0 | runtime |
| core-foundation | 0.10.1 | MIT OR Apache-2.0 | runtime |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | runtime |
| core-graphics | 0.25.0 | MIT OR Apache-2.0 | runtime |
| core-graphics-types | 0.2.0 | MIT OR Apache-2.0 | runtime |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | runtime |
| cpufeatures | 0.3.0 | MIT OR Apache-2.0 | runtime |
| crc32fast | 1.5.0 | MIT OR Apache-2.0 | runtime |
| crossbeam-channel | 0.5.16 | MIT OR Apache-2.0 | runtime |
| crossbeam-deque | 0.8.7 | MIT OR Apache-2.0 | runtime |
| crossbeam-epoch | 0.9.20 | MIT OR Apache-2.0 | runtime |
| crossbeam-utils | 0.8.22 | MIT OR Apache-2.0 | runtime |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | runtime |
| cssparser | 0.36.0 | MPL-2.0 | runtime |
| cssparser-macros | 0.6.1 | MPL-2.0 | runtime |
| ctor | 0.8.0 | Apache-2.0 OR MIT | runtime |
| ctor-proc-macro | 0.0.7 | Apache-2.0 OR MIT | runtime |
| ctr | 0.9.2 | MIT OR Apache-2.0 | runtime |
| darling | 0.23.0 | MIT | runtime |
| darling_core | 0.23.0 | MIT | runtime |
| darling_macro | 0.23.0 | MIT | runtime |
| dbus | 0.9.12 | Apache-2.0 OR MIT | runtime |
| defmt | 1.1.1 | MIT OR Apache-2.0 | runtime |
| defmt-macros | 1.1.1 | MIT OR Apache-2.0 | runtime |
| defmt-parser | 1.0.0 | MIT OR Apache-2.0 | runtime |
| deranged | 0.5.8 | MIT OR Apache-2.0 | runtime |
| derive_arbitrary | 1.4.2 | MIT OR Apache-2.0 | runtime |
| derive_more | 2.1.1 | MIT | runtime |
| derive_more-impl | 2.1.1 | MIT | runtime |
| digest | 0.10.7 | MIT OR Apache-2.0 | runtime |
| dirs | 6.0.0 | MIT OR Apache-2.0 | runtime |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 | runtime |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | runtime |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | runtime |
| dlopen2 | 0.8.2 | MIT | runtime |
| dlopen2_derive | 0.4.3 | MIT | runtime |
| dom_query | 0.27.0 | MIT | runtime |
| dpi | 0.1.2 | Apache-2.0 AND MIT | runtime |
| dtoa | 1.0.11 | MIT OR Apache-2.0 | runtime |
| dtoa-short | 0.3.5 | MPL-2.0 | runtime |
| dtor | 0.3.0 | Apache-2.0 OR MIT | runtime |
| dtor-proc-macro | 0.0.6 | Apache-2.0 OR MIT | runtime |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 | runtime |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 | runtime |
| either | 1.17.0 | MIT OR Apache-2.0 | runtime |
| embed-resource | 3.0.11 | MIT | runtime |
| embed_plist | 1.2.2 | MIT OR Apache-2.0 | runtime |
| encoding_rs | 0.8.35 | (Apache-2.0 OR MIT) AND BSD-3-Clause | runtime |
| endi | 1.1.1 | MIT | runtime |
| enumflags2 | 0.7.12 | MIT OR Apache-2.0 | runtime |
| enumflags2_derive | 0.7.12 | MIT OR Apache-2.0 | runtime |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | runtime |
| erased-serde | 0.4.10 | MIT OR Apache-2.0 | runtime |
| errno | 0.3.14 | MIT OR Apache-2.0 | runtime |
| event-listener | 5.4.2 | Apache-2.0 OR MIT | runtime |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT | runtime |
| fallible-iterator | 0.3.0 | MIT OR Apache-2.0 | runtime |
| fallible-streaming-iterator | 0.1.9 | MIT OR Apache-2.0 | runtime |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | runtime |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | runtime |
| field-offset | 0.3.6 | MIT OR Apache-2.0 | runtime |
| filetime | 0.2.29 | MIT OR Apache-2.0 | runtime |
| find-msvc-tools | 0.1.10 | MIT OR Apache-2.0 | runtime |
| flate2 | 1.1.9 | MIT OR Apache-2.0 | runtime |
| fnv | 1.0.7 | Apache-2.0 OR MIT | runtime |
| foldhash | 0.2.0 | Zlib | runtime |
| foreign-types | 0.5.0 | MIT OR Apache-2.0 | runtime |
| foreign-types-macros | 0.2.4 | MIT OR Apache-2.0 | runtime |
| foreign-types-shared | 0.3.1 | MIT OR Apache-2.0 | runtime |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | runtime |
| futures-channel | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-core | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-executor | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-io | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-lite | 2.6.1 | Apache-2.0 OR MIT | runtime |
| futures-macro | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-sink | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-task | 0.3.33 | MIT OR Apache-2.0 | runtime |
| futures-util | 0.3.33 | MIT OR Apache-2.0 | runtime |
| gdk | 0.18.2 | MIT | runtime |
| gdk-pixbuf | 0.18.5 | MIT | runtime |
| gdk-pixbuf-sys | 0.18.0 | MIT | runtime |
| gdk-sys | 0.18.2 | MIT | runtime |
| gdkwayland-sys | 0.18.2 | MIT | runtime |
| gdkx11 | 0.18.2 | MIT | runtime |
| gdkx11-sys | 0.18.2 | MIT | runtime |
| generic-array | 0.14.7 | MIT | runtime |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | runtime |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | runtime |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | runtime |
| ghash | 0.5.1 | Apache-2.0 OR MIT | runtime |
| gio | 0.18.4 | MIT | runtime |
| gio-sys | 0.18.1 | MIT | runtime |
| glib | 0.18.5 | MIT | runtime |
| glib-macros | 0.18.5 | MIT | runtime |
| glib-sys | 0.18.1 | MIT | runtime |
| glob | 0.3.4 | MIT OR Apache-2.0 | runtime |
| gobject-sys | 0.18.0 | MIT | runtime |
| gtk | 0.18.2 | MIT | runtime |
| gtk-sys | 0.18.2 | MIT | runtime |
| gtk3-macros | 0.18.2 | MIT | runtime |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 | runtime |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | runtime |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | runtime |
| hashlink | 0.12.1 | MIT OR Apache-2.0 | runtime |
| heck | 0.4.1 | MIT OR Apache-2.0 | runtime |
| heck | 0.5.0 | MIT OR Apache-2.0 | runtime |
| hermit-abi | 0.5.2 | MIT OR Apache-2.0 | runtime |
| hex | 0.4.3 | MIT OR Apache-2.0 | runtime |
| hmac | 0.12.1 | MIT OR Apache-2.0 | runtime |
| html5ever | 0.38.0 | MIT OR Apache-2.0 | runtime |
| http | 1.5.0 | MIT OR Apache-2.0 | runtime |
| http-body | 1.1.0 | MIT | runtime |
| http-body-util | 0.1.4 | MIT | runtime |
| httparse | 1.10.1 | MIT OR Apache-2.0 | runtime |
| hyper | 1.11.0 | MIT | runtime |
| hyper-rustls | 0.27.9 | Apache-2.0 OR ISC OR MIT | runtime |
| hyper-util | 0.1.20 | MIT | runtime |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | runtime |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 | runtime |
| ico | 0.5.0 | MIT | runtime |
| icu_collections | 2.2.0 | Unicode-3.0 | runtime |
| icu_locale_core | 2.2.0 | Unicode-3.0 | runtime |
| icu_normalizer | 2.2.0 | Unicode-3.0 | runtime |
| icu_normalizer_data | 2.2.0 | Unicode-3.0 | runtime |
| icu_properties | 2.2.0 | Unicode-3.0 | runtime |
| icu_properties_data | 2.2.0 | Unicode-3.0 | runtime |
| icu_provider | 2.2.0 | Unicode-3.0 | runtime |
| ident_case | 1.0.1 | MIT OR Apache-2.0 | runtime |
| idna | 1.1.0 | MIT OR Apache-2.0 | runtime |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | runtime |
| indexmap | 1.9.3 | Apache-2.0 OR MIT | runtime |
| indexmap | 2.14.0 | Apache-2.0 OR MIT | runtime |
| infer | 0.19.0 | MIT | runtime |
| inout | 0.1.4 | MIT OR Apache-2.0 | runtime |
| ipnet | 2.12.1 | MIT OR Apache-2.0 | runtime |
| is-docker | 0.2.0 | MIT | runtime |
| is-wsl | 0.4.0 | MIT | runtime |
| itoa | 1.0.18 | MIT OR Apache-2.0 | runtime |
| javascriptcore-rs | 1.1.2 | MIT | runtime |
| javascriptcore-rs-sys | 1.1.1 | MIT | runtime |
| jiff | 0.2.35 | Unlicense OR MIT | runtime |
| jiff-core | 0.1.0 | Unlicense OR MIT | runtime |
| jiff-static | 0.2.35 | Unlicense OR MIT | runtime |
| jiff-tzdb | 0.1.8 | Unlicense OR MIT | runtime |
| jiff-tzdb-platform | 0.1.3 | Unlicense OR MIT | runtime |
| jni | 0.21.1 | MIT OR Apache-2.0 | runtime |
| jni | 0.22.4 | MIT OR Apache-2.0 | runtime |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 | runtime |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 | runtime |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | runtime |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 | runtime |
| js-sys | 0.3.104 | MIT OR Apache-2.0 | runtime |
| json-patch | 3.0.1 | MIT OR Apache-2.0 | runtime |
| jsonptr | 0.6.3 | MIT OR Apache-2.0 | runtime |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 | runtime |
| libappindicator | 0.9.0 | Apache-2.0 OR MIT | runtime |
| libappindicator-sys | 0.9.0 | Apache-2.0 OR MIT | runtime |
| libc | 0.2.189 | MIT OR Apache-2.0 | runtime |
| libdbus-sys | 0.2.7 | Apache-2.0 OR MIT | runtime |
| libloading | 0.7.4 | ISC | runtime |
| libredox | 0.1.19 | MIT | runtime |
| libsqlite3-sys | 0.38.2 | MIT | runtime |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | runtime |
| litemap | 0.8.2 | Unicode-3.0 | runtime |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | runtime |
| log | 0.4.33 | MIT OR Apache-2.0 | runtime |
| lru-slab | 0.1.2 | MIT OR Apache-2.0 OR Zlib | runtime |
| markup5ever | 0.38.0 | MIT OR Apache-2.0 | runtime |
| memchr | 2.8.3 | Unlicense OR MIT | runtime |
| memoffset | 0.9.1 | MIT | runtime |
| mime | 0.3.17 | MIT OR Apache-2.0 | runtime |
| minisign-verify | 0.2.5 | MIT | runtime |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | runtime |
| mio | 1.2.2 | MIT | runtime |
| muda | 0.19.3 | Apache-2.0 OR MIT | runtime |
| ndk | 0.9.0 | MIT OR Apache-2.0 | runtime |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | runtime |
| new_debug_unreachable | 1.0.6 | MIT | runtime |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | runtime |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | runtime |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | runtime |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | runtime |
| objc2 | 0.6.4 | MIT | runtime |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-cloud-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-core-data | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-core-image | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-core-location | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-core-text | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-encode | 4.1.0 | MIT | runtime |
| objc2-exception-helper | 0.1.1 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-foundation | 0.3.2 | MIT | runtime |
| objc2-io-surface | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-osa-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-quartz-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-ui-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-user-notifications | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| objc2-web-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | runtime |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | runtime |
| opaque-debug | 0.3.1 | MIT OR Apache-2.0 | runtime |
| open | 5.4.1 | MIT | runtime |
| openssl-probe | 0.2.1 | MIT OR Apache-2.0 | runtime |
| option-ext | 0.2.0 | MPL-2.0 | runtime |
| ordered-stream | 0.2.0 | MIT OR Apache-2.0 | runtime |
| os_pipe | 1.2.3 | MIT | runtime |
| osakit | 0.3.1 | MIT OR Apache-2.0 | runtime |
| pango | 0.18.3 | MIT | runtime |
| pango-sys | 0.18.0 | MIT | runtime |
| parking | 2.2.1 | Apache-2.0 OR MIT | runtime |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | runtime |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | runtime |
| pbkdf2 | 0.12.2 | MIT OR Apache-2.0 | runtime |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | runtime |
| phf | 0.13.1 | MIT | runtime |
| phf_codegen | 0.13.1 | MIT | runtime |
| phf_generator | 0.13.1 | MIT | runtime |
| phf_macros | 0.13.1 | MIT | runtime |
| phf_shared | 0.13.1 | MIT | runtime |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | runtime |
| piper | 0.2.5 | MIT OR Apache-2.0 | runtime |
| pkg-config | 0.3.33 | MIT OR Apache-2.0 | runtime |
| plist | 1.10.0 | MIT | runtime |
| png | 0.17.16 | MIT OR Apache-2.0 | runtime |
| png | 0.18.1 | MIT OR Apache-2.0 | runtime |
| polling | 3.11.0 | Apache-2.0 OR MIT | runtime |
| polyval | 0.6.2 | Apache-2.0 OR MIT | runtime |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | runtime |
| portable-atomic-util | 0.2.7 | Apache-2.0 OR MIT | runtime |
| potential_utf | 0.1.5 | Unicode-3.0 | runtime |
| powerfmt | 0.2.0 | MIT OR Apache-2.0 | runtime |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 | runtime |
| precomputed-hash | 0.1.1 | MIT | runtime |
| proc-macro-crate | 1.3.1 | MIT OR Apache-2.0 | runtime |
| proc-macro-crate | 2.0.2 | MIT OR Apache-2.0 | runtime |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | runtime |
| proc-macro-error | 1.0.4 | MIT OR Apache-2.0 | runtime |
| proc-macro-error-attr | 1.0.4 | MIT OR Apache-2.0 | runtime |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | runtime |
| quick-xml | 0.41.0 | MIT | runtime |
| quinn | 0.11.11 | MIT OR Apache-2.0 | runtime |
| quinn-proto | 0.11.16 | MIT OR Apache-2.0 | runtime |
| quinn-udp | 0.5.15 | MIT OR Apache-2.0 | runtime |
| quote | 1.0.47 | MIT OR Apache-2.0 | runtime |
| r-efi | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | runtime |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | runtime |
| rand | 0.10.2 | MIT OR Apache-2.0 | runtime |
| rand | 0.8.7 | MIT OR Apache-2.0 | runtime |
| rand_chacha | 0.3.1 | MIT OR Apache-2.0 | runtime |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | runtime |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | runtime |
| rand_pcg | 0.10.2 | MIT OR Apache-2.0 | runtime |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | runtime |
| rayon | 1.12.0 | MIT OR Apache-2.0 | runtime |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | runtime |
| redox_syscall | 0.5.18 | MIT | runtime |
| redox_users | 0.5.2 | MIT | runtime |
| ref-cast | 1.0.26 | MIT OR Apache-2.0 | runtime |
| ref-cast-impl | 1.0.26 | MIT OR Apache-2.0 | runtime |
| regex | 1.13.1 | MIT OR Apache-2.0 | runtime |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | runtime |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | runtime |
| reqwest | 0.12.28 | MIT OR Apache-2.0 | runtime |
| reqwest | 0.13.4 | MIT OR Apache-2.0 | runtime |
| rfd | 0.16.0 | MIT | runtime |
| ring | 0.17.14 | Apache-2.0 AND ISC | runtime |
| rsqlite-vfs | 0.1.1 | MIT | runtime |
| rusqlite | 0.40.2 | MIT | runtime |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | runtime |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | runtime |
| rustix | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | runtime |
| rustls | 0.23.43 | Apache-2.0 OR ISC OR MIT | runtime |
| rustls-native-certs | 0.8.4 | Apache-2.0 OR ISC OR MIT | runtime |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | runtime |
| rustls-platform-verifier | 0.7.0 | MIT OR Apache-2.0 | runtime |
| rustls-platform-verifier-android | 0.1.1 | MIT OR Apache-2.0 | runtime |
| rustls-webpki | 0.103.13 | ISC | runtime |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | runtime |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | runtime |
| same-file | 1.0.6 | Unlicense OR MIT | runtime |
| schannel | 0.1.29 | MIT | runtime |
| schemars | 0.8.22 | MIT | runtime |
| schemars | 0.9.0 | MIT | runtime |
| schemars | 1.2.2 | MIT | runtime |
| schemars_derive | 0.8.22 | MIT | runtime |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | runtime |
| security-framework | 3.7.0 | MIT OR Apache-2.0 | runtime |
| security-framework-sys | 2.17.0 | MIT OR Apache-2.0 | runtime |
| selectors | 0.36.1 | MPL-2.0 | runtime |
| semver | 1.0.28 | MIT OR Apache-2.0 | runtime |
| serde | 1.0.229 | MIT OR Apache-2.0 | runtime |
| serde-untagged | 0.1.9 | MIT OR Apache-2.0 | runtime |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | runtime |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | runtime |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 | runtime |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | runtime |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 | runtime |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | runtime |
| serde_spanned | 1.1.1 | MIT OR Apache-2.0 | runtime |
| serde_urlencoded | 0.7.1 | MIT OR Apache-2.0 | runtime |
| serde_with | 3.22.0 | MIT OR Apache-2.0 | runtime |
| serde_with_macros | 3.22.0 | MIT OR Apache-2.0 | runtime |
| serialize-to-javascript | 0.1.2 | MIT OR Apache-2.0 | runtime |
| serialize-to-javascript-impl | 0.1.2 | MIT OR Apache-2.0 | runtime |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 | runtime |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | runtime |
| shared_child | 1.1.1 | MIT | runtime |
| shlex | 2.0.1 | MIT OR Apache-2.0 | runtime |
| sigchld | 0.2.4 | MIT | runtime |
| signal-hook | 0.3.18 | Apache-2.0 OR MIT | runtime |
| signal-hook-registry | 1.4.8 | MIT OR Apache-2.0 | runtime |
| simd-adler32 | 0.3.10 | MIT | runtime |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT | runtime |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | runtime |
| siphasher | 1.0.3 | MIT OR Apache-2.0 | runtime |
| slab | 0.4.12 | MIT | runtime |
| smallvec | 1.15.2 | MIT OR Apache-2.0 | runtime |
| socket2 | 0.6.5 | MIT OR Apache-2.0 | runtime |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 | runtime |
| soup3 | 0.5.0 | MIT | runtime |
| soup3-sys | 0.5.0 | MIT | runtime |
| sqlite-wasm-rs | 0.5.5 | MIT | runtime |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | runtime |
| string_cache | 0.9.0 | MIT OR Apache-2.0 | runtime |
| string_cache_codegen | 0.6.1 | MIT OR Apache-2.0 | runtime |
| strsim | 0.11.1 | MIT | runtime |
| subtle | 2.6.1 | BSD-3-Clause | runtime |
| swift-rs | 1.0.7 | MIT OR Apache-2.0 | runtime |
| syn | 1.0.109 | MIT OR Apache-2.0 | runtime |
| syn | 2.0.119 | MIT OR Apache-2.0 | runtime |
| syn | 3.0.3 | MIT OR Apache-2.0 | runtime |
| sync_wrapper | 1.0.2 | Apache-2.0 | runtime |
| synstructure | 0.13.2 | MIT | runtime |
| system-deps | 6.2.2 | MIT OR Apache-2.0 | runtime |
| tao | 0.35.3 | Apache-2.0 | runtime |
| tao-macros | 0.1.4 | MIT OR Apache-2.0 | runtime |
| tar | 0.4.46 | MIT OR Apache-2.0 | runtime |
| target-lexicon | 0.12.16 | Apache-2.0 WITH LLVM-exception | runtime |
| tauri | 2.11.5 | Apache-2.0 OR MIT | runtime |
| tauri-build | 2.6.3 | Apache-2.0 OR MIT | runtime |
| tauri-codegen | 2.6.3 | Apache-2.0 OR MIT | runtime |
| tauri-macros | 2.6.3 | Apache-2.0 OR MIT | runtime |
| tauri-plugin | 2.6.3 | Apache-2.0 OR MIT | runtime |
| tauri-plugin-dialog | 2.7.2 | Apache-2.0 OR MIT | runtime |
| tauri-plugin-fs | 2.5.1 | Apache-2.0 OR MIT | runtime |
| tauri-plugin-opener | 2.5.4 | Apache-2.0 OR MIT | runtime |
| tauri-plugin-process | 2.3.1 | Apache-2.0 OR MIT | runtime |
| tauri-plugin-shell | 2.3.5 | Apache-2.0 OR MIT | runtime |
| tauri-plugin-updater | 2.10.1 | Apache-2.0 OR MIT | runtime |
| tauri-runtime | 2.11.3 | Apache-2.0 OR MIT | runtime |
| tauri-runtime-wry | 2.11.4 | Apache-2.0 OR MIT | runtime |
| tauri-utils | 2.9.3 | Apache-2.0 OR MIT | runtime |
| tauri-winres | 0.3.6 | MIT | runtime |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | runtime |
| tendril | 0.5.1 | MIT OR Apache-2.0 | runtime |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | runtime |
| thiserror | 2.0.20 | MIT OR Apache-2.0 | runtime |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | runtime |
| thiserror-impl | 2.0.20 | MIT OR Apache-2.0 | runtime |
| time | 0.3.55 | MIT OR Apache-2.0 | runtime |
| time-core | 0.1.9 | MIT OR Apache-2.0 | runtime |
| time-macros | 0.2.32 | MIT OR Apache-2.0 | runtime |
| tinystr | 0.8.3 | Unicode-3.0 | runtime |
| tinyvec | 1.12.0 | Zlib OR Apache-2.0 OR MIT | runtime |
| tinyvec_macros | 0.1.1 | MIT OR Apache-2.0 OR Zlib | runtime |
| tokio | 1.53.1 | MIT | runtime |
| tokio-macros | 2.7.2 | MIT | runtime |
| tokio-rustls | 0.26.4 | MIT OR Apache-2.0 | runtime |
| tokio-util | 0.7.19 | MIT | runtime |
| toml | 0.8.2 | MIT OR Apache-2.0 | runtime |
| toml | 0.9.12+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| toml | 1.1.4+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| toml_datetime | 0.6.3 | MIT OR Apache-2.0 | runtime |
| toml_datetime | 0.7.5+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| toml_datetime | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| toml_edit | 0.19.15 | MIT OR Apache-2.0 | runtime |
| toml_edit | 0.20.2 | MIT OR Apache-2.0 | runtime |
| toml_edit | 0.25.13+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| toml_parser | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| toml_writer | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | runtime |
| tower | 0.5.3 | MIT | runtime |
| tower-http | 0.6.11 | MIT | runtime |
| tower-layer | 0.3.3 | MIT | runtime |
| tower-service | 0.3.3 | MIT | runtime |
| tracing | 0.1.44 | MIT | runtime |
| tracing-attributes | 0.1.31 | MIT | runtime |
| tracing-core | 0.1.36 | MIT | runtime |
| tray-icon | 0.24.2 | MIT OR Apache-2.0 | runtime |
| try-lock | 0.2.5 | MIT | runtime |
| typeid | 1.0.3 | MIT OR Apache-2.0 | runtime |
| typenum | 1.20.1 | MIT OR Apache-2.0 | runtime |
| uds_windows | 1.2.1 | MIT | runtime |
| unic-char-property | 0.9.0 | MIT OR Apache-2.0 | runtime |
| unic-char-range | 0.9.0 | MIT OR Apache-2.0 | runtime |
| unic-common | 0.9.0 | MIT OR Apache-2.0 | runtime |
| unic-ucd-ident | 0.9.0 | MIT OR Apache-2.0 | runtime |
| unic-ucd-version | 0.9.0 | MIT OR Apache-2.0 | runtime |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 | runtime |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | runtime |
| universal-hash | 0.5.1 | MIT OR Apache-2.0 | runtime |
| untrusted | 0.9.0 | ISC | runtime |
| url | 2.5.8 | MIT OR Apache-2.0 | runtime |
| urlpattern | 0.3.0 | MIT | runtime |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | runtime |
| uuid | 1.24.0 | Apache-2.0 OR MIT | runtime |
| vcpkg | 0.2.15 | MIT OR Apache-2.0 | runtime |
| version-compare | 0.2.1 | MIT | runtime |
| version_check | 0.9.5 | MIT OR Apache-2.0 | runtime |
| vswhom | 0.1.0 | MIT | runtime |
| vswhom-sys | 0.1.3 | MIT | runtime |
| walkdir | 2.5.0 | Unlicense OR MIT | runtime |
| want | 0.3.1 | MIT | runtime |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | runtime |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | runtime |
| wasm-bindgen | 0.2.127 | MIT OR Apache-2.0 | runtime |
| wasm-bindgen-futures | 0.4.77 | MIT OR Apache-2.0 | runtime |
| wasm-bindgen-macro | 0.2.127 | MIT OR Apache-2.0 | runtime |
| wasm-bindgen-macro-support | 0.2.127 | MIT OR Apache-2.0 | runtime |
| wasm-bindgen-shared | 0.2.127 | MIT OR Apache-2.0 | runtime |
| wasm-streams | 0.5.0 | MIT OR Apache-2.0 | runtime |
| web-sys | 0.3.104 | MIT OR Apache-2.0 | runtime |
| web-time | 1.1.0 | MIT OR Apache-2.0 | runtime |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 | runtime |
| webkit2gtk | 2.0.2 | MIT | runtime |
| webkit2gtk-sys | 2.0.2 | MIT | runtime |
| webpki-root-certs | 1.0.9 | CDLA-Permissive-2.0 | runtime |
| webpki-roots | 1.0.9 | CDLA-Permissive-2.0 | runtime |
| webview2-com | 0.38.2 | MIT | runtime |
| webview2-com-macros | 0.8.1 | MIT | runtime |
| webview2-com-sys | 0.38.2 | MIT | runtime |
| winapi | 0.3.9 | MIT OR Apache-2.0 | runtime |
| winapi-i686-pc-windows-gnu | 0.4.0 | MIT OR Apache-2.0 | runtime |
| winapi-util | 0.1.11 | Unlicense OR MIT | runtime |
| winapi-x86_64-pc-windows-gnu | 0.4.0 | MIT OR Apache-2.0 | runtime |
| window-vibrancy | 0.6.0 | Apache-2.0 OR MIT | runtime |
| windows | 0.61.3 | MIT OR Apache-2.0 | runtime |
| windows-collections | 0.2.0 | MIT OR Apache-2.0 | runtime |
| windows-core | 0.61.2 | MIT OR Apache-2.0 | runtime |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | runtime |
| windows-future | 0.2.1 | MIT OR Apache-2.0 | runtime |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | runtime |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | runtime |
| windows-link | 0.1.3 | MIT OR Apache-2.0 | runtime |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | runtime |
| windows-numerics | 0.2.0 | MIT OR Apache-2.0 | runtime |
| windows-result | 0.3.4 | MIT OR Apache-2.0 | runtime |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | runtime |
| windows-strings | 0.4.2 | MIT OR Apache-2.0 | runtime |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | runtime |
| windows-sys | 0.45.0 | MIT OR Apache-2.0 | runtime |
| windows-sys | 0.52.0 | MIT OR Apache-2.0 | runtime |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 | runtime |
| windows-sys | 0.60.2 | MIT OR Apache-2.0 | runtime |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | runtime |
| windows-targets | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows-targets | 0.53.5 | MIT OR Apache-2.0 | runtime |
| windows-threading | 0.1.0 | MIT OR Apache-2.0 | runtime |
| windows-version | 0.1.7 | MIT OR Apache-2.0 | runtime |
| windows_aarch64_gnullvm | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_aarch64_gnullvm | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_aarch64_msvc | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_aarch64_msvc | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_i686_gnu | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_i686_gnu | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_i686_gnullvm | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_i686_msvc | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_i686_msvc | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_gnu | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_gnu | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_gnullvm | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_gnullvm | 0.53.1 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_msvc | 0.42.2 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | runtime |
| windows_x86_64_msvc | 0.53.1 | MIT OR Apache-2.0 | runtime |
| winnow | 0.5.40 | MIT | runtime |
| winnow | 0.7.15 | MIT | runtime |
| winnow | 1.0.4 | MIT | runtime |
| winreg | 0.55.0 | MIT | runtime |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | runtime |
| writeable | 0.6.3 | Unicode-3.0 | runtime |
| wry | 0.55.1 | Apache-2.0 OR MIT | runtime |
| x11 | 2.21.0 | MIT | runtime |
| x11-dl | 2.21.0 | MIT | runtime |
| xattr | 1.6.1 | MIT OR Apache-2.0 | runtime |
| yoke | 0.8.3 | Unicode-3.0 | runtime |
| yoke-derive | 0.8.2 | Unicode-3.0 | runtime |
| zbus | 5.19.0 | MIT | runtime |
| zbus_macros | 5.19.0 | MIT | runtime |
| zbus_names | 4.3.4 | MIT | runtime |
| zcheapstr | 1.1.0 | MIT | runtime |
| zerocopy | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT | runtime |
| zerocopy-derive | 0.8.56 | BSD-2-Clause OR Apache-2.0 OR MIT | runtime |
| zerofrom | 0.1.8 | Unicode-3.0 | runtime |
| zerofrom-derive | 0.1.7 | Unicode-3.0 | runtime |
| zeroize | 1.9.0 | Apache-2.0 OR MIT | runtime |
| zerotrie | 0.2.4 | Unicode-3.0 | runtime |
| zerovec | 0.11.6 | Unicode-3.0 | runtime |
| zerovec-derive | 0.11.3 | Unicode-3.0 | runtime |
| zip | 4.6.1 | MIT | runtime |
| zmij | 1.0.23 | MIT | runtime |
| zvariant | 5.14.0 | MIT | runtime |
| zvariant_derive | 5.14.0 | MIT | runtime |
| zvariant_utils | 4.0.0 | MIT | runtime |

## npm 依赖（136）

| 包 | 版本 | 许可证 | 范围 |
|---|---|---|---|
| @babel/code-frame | 7.29.7 | MIT | dev |
| @babel/compat-data | 7.29.7 | MIT | dev |
| @babel/core | 7.29.7 | MIT | dev |
| @babel/generator | 7.29.8 | MIT | dev |
| @babel/helper-compilation-targets | 7.29.7 | MIT | dev |
| @babel/helper-globals | 7.29.7 | MIT | dev |
| @babel/helper-module-imports | 7.29.7 | MIT | dev |
| @babel/helper-module-transforms | 7.29.7 | MIT | dev |
| @babel/helper-plugin-utils | 7.29.7 | MIT | dev |
| @babel/helper-string-parser | 7.29.7 | MIT | dev |
| @babel/helper-validator-identifier | 7.29.7 | MIT | dev |
| @babel/helper-validator-option | 7.29.7 | MIT | dev |
| @babel/helpers | 7.29.7 | MIT | dev |
| @babel/parser | 7.29.8 | MIT | dev |
| @babel/plugin-transform-react-jsx-self | 7.29.7 | MIT | dev |
| @babel/plugin-transform-react-jsx-source | 7.29.7 | MIT | dev |
| @babel/template | 7.29.7 | MIT | dev |
| @babel/traverse | 7.29.8 | MIT | dev |
| @babel/types | 7.29.8 | MIT | dev |
| @esbuild/aix-ppc64 | 0.28.2 | MIT | dev |
| @esbuild/android-arm | 0.28.2 | MIT | dev |
| @esbuild/android-arm64 | 0.28.2 | MIT | dev |
| @esbuild/android-x64 | 0.28.2 | MIT | dev |
| @esbuild/darwin-arm64 | 0.28.2 | MIT | dev |
| @esbuild/darwin-x64 | 0.28.2 | MIT | dev |
| @esbuild/freebsd-arm64 | 0.28.2 | MIT | dev |
| @esbuild/freebsd-x64 | 0.28.2 | MIT | dev |
| @esbuild/linux-arm | 0.28.2 | MIT | dev |
| @esbuild/linux-arm64 | 0.28.2 | MIT | dev |
| @esbuild/linux-ia32 | 0.28.2 | MIT | dev |
| @esbuild/linux-loong64 | 0.28.2 | MIT | dev |
| @esbuild/linux-mips64el | 0.28.2 | MIT | dev |
| @esbuild/linux-ppc64 | 0.28.2 | MIT | dev |
| @esbuild/linux-riscv64 | 0.28.2 | MIT | dev |
| @esbuild/linux-s390x | 0.28.2 | MIT | dev |
| @esbuild/linux-x64 | 0.28.2 | MIT | dev |
| @esbuild/netbsd-arm64 | 0.28.2 | MIT | dev |
| @esbuild/netbsd-x64 | 0.28.2 | MIT | dev |
| @esbuild/openbsd-arm64 | 0.28.2 | MIT | dev |
| @esbuild/openbsd-x64 | 0.28.2 | MIT | dev |
| @esbuild/openharmony-arm64 | 0.28.2 | MIT | dev |
| @esbuild/sunos-x64 | 0.28.2 | MIT | dev |
| @esbuild/win32-arm64 | 0.28.2 | MIT | dev |
| @esbuild/win32-ia32 | 0.28.2 | MIT | dev |
| @esbuild/win32-x64 | 0.28.2 | MIT | dev |
| @jridgewell/gen-mapping | 0.3.13 | MIT | dev |
| @jridgewell/remapping | 2.3.5 | MIT | dev |
| @jridgewell/resolve-uri | 3.1.2 | MIT | dev |
| @jridgewell/sourcemap-codec | 1.5.5 | MIT | dev |
| @jridgewell/trace-mapping | 0.3.31 | MIT | dev |
| @napi-rs/lzma-linux-x64-gnu | 1.5.1 | MIT | dev |
| @rolldown/pluginutils | 1.0.0-beta.27 | MIT | dev |
| @rollup/rollup-android-arm-eabi | 4.62.4 | MIT | dev |
| @rollup/rollup-android-arm64 | 4.62.4 | MIT | dev |
| @rollup/rollup-darwin-arm64 | 4.62.4 | MIT | dev |
| @rollup/rollup-darwin-x64 | 4.62.4 | MIT | dev |
| @rollup/rollup-freebsd-arm64 | 4.62.4 | MIT | dev |
| @rollup/rollup-freebsd-x64 | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-arm-gnueabihf | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-arm-musleabihf | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-arm64-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-arm64-musl | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-loong64-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-loong64-musl | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-ppc64-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-ppc64-musl | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-riscv64-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-riscv64-musl | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-s390x-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-x64-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-linux-x64-musl | 4.62.4 | MIT | dev |
| @rollup/rollup-openbsd-x64 | 4.62.4 | MIT | dev |
| @rollup/rollup-openharmony-arm64 | 4.62.4 | MIT | dev |
| @rollup/rollup-win32-arm64-msvc | 4.62.4 | MIT | dev |
| @rollup/rollup-win32-ia32-msvc | 4.62.4 | MIT | dev |
| @rollup/rollup-win32-x64-gnu | 4.62.4 | MIT | dev |
| @rollup/rollup-win32-x64-msvc | 4.62.4 | MIT | dev |
| @tauri-apps/api | 2.11.1 | Apache-2.0 OR MIT | runtime |
| @tauri-apps/cli | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-darwin-arm64 | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-darwin-x64 | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-linux-arm-gnueabihf | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-linux-arm64-gnu | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-linux-arm64-musl | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-linux-riscv64-gnu | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-linux-x64-gnu | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-linux-x64-musl | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-win32-arm64-msvc | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-win32-ia32-msvc | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/cli-win32-x64-msvc | 2.11.4 | Apache-2.0 OR MIT | dev |
| @tauri-apps/plugin-opener | 2.5.4 | MIT OR Apache-2.0 | runtime |
| @tauri-apps/plugin-process | 2.3.1 | MIT OR Apache-2.0 | runtime |
| @tauri-apps/plugin-updater | 2.10.1 | MIT OR Apache-2.0 | runtime |
| @types/babel__core | 7.20.5 | MIT | dev |
| @types/babel__generator | 7.27.0 | MIT | dev |
| @types/babel__template | 7.4.4 | MIT | dev |
| @types/babel__traverse | 7.28.0 | MIT | dev |
| @types/estree | 1.0.9 | MIT | dev |
| @types/react | 19.2.18 | MIT | dev |
| @types/react-dom | 19.2.4 | MIT | dev |
| @vitejs/plugin-react | 4.7.0 | MIT | dev |
| baseline-browser-mapping | 2.11.13 | Apache-2.0 | dev |
| browserslist | 4.28.8 | MIT | dev |
| caniuse-lite | 1.0.30001809 | CC-BY-4.0 | dev |
| convert-source-map | 2.0.0 | MIT | dev |
| csstype | 3.2.3 | MIT | dev |
| debug | 4.4.3 | MIT | dev |
| electron-to-chromium | 1.5.403 | ISC | dev |
| esbuild | 0.28.2 | MIT | dev |
| escalade | 3.2.0 | MIT | dev |
| fdir | 6.5.0 | MIT | dev |
| fsevents | 2.3.3 | MIT | dev |
| gensync | 1.0.0-beta.2 | MIT | dev |
| js-tokens | 4.0.0 | MIT | dev |
| jsesc | 3.1.0 | MIT | dev |
| json5 | 2.2.3 | MIT | dev |
| lru-cache | 5.1.1 | ISC | dev |
| lucide-react | 1.31.0 | ISC | runtime |
| ms | 2.1.3 | MIT | dev |
| nanoid | 3.3.18 | MIT | dev |
| node-releases | 2.0.53 | MIT | dev |
| picocolors | 1.1.1 | ISC | dev |
| picomatch | 4.0.5 | MIT | dev |
| postcss | 8.5.26 | MIT | dev |
| react | 19.2.8 | MIT | runtime |
| react-dom | 19.2.8 | MIT | runtime |
| react-refresh | 0.17.0 | MIT | dev |
| rollup | 4.62.4 | MIT | dev |
| scheduler | 0.27.0 | MIT | runtime |
| semver | 6.3.1 | ISC | dev |
| source-map-js | 1.2.1 | BSD-3-Clause | dev |
| tinyglobby | 0.2.17 | MIT | dev |
| typescript | 5.8.3 | Apache-2.0 | dev |
| update-browserslist-db | 1.3.1 | MIT | dev |
| vite | 7.3.6 | MIT | dev |
| yallist | 3.1.1 | ISC | dev |

## python 依赖（44）

| 包 | 版本 | 许可证 | 范围 |
|---|---|---|---|
| altgraph | 0.17.5 | MIT | build |
| annotated-doc | 0.0.5 | MIT | runtime |
| annotated-types | 0.8.0 | MIT | runtime |
| anyio | 4.16.0 | MIT | runtime |
| click | 8.5.1 | BSD-3-Clause | runtime |
| colorama | 0.4.6 | BSD-3-Clause（人工核实） | runtime |
| et-xmlfile | 2.0.0 | MIT | runtime |
| fastapi | 0.162.2 | MIT | runtime |
| filelock | 4.2.9 | MIT | runtime |
| flatbuffers | 25.12.19 | Apache-2.0 | runtime |
| fsspec | 2026.9.0 | BSD-3-Clause | runtime |
| h11 | 0.16.0 | MIT | runtime |
| hf-xet | 1.8.0 | Apache-2.0 | runtime |
| httpcore2 | 2.16.0 | BSD-3-Clause | runtime |
| httpx2 | 2.16.0 | BSD-3-Clause | runtime |
| huggingface-hub | 2.4.0 | Apache-2.0 | runtime |
| idna | 3.20 | BSD-3-Clause | runtime |
| lxml | 6.1.3 | BSD-3-Clause | runtime |
| macholib | 1.16.4 | MIT | build |
| numpy | 2.4.6 | BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0 | runtime |
| onnxruntime | 1.31.1 | MIT | runtime |
| openpyxl | 3.1.5 | MIT | runtime |
| opentelemetry-api | 1.45.2 | Apache-2.0 | runtime |
| packaging | 26.3 | Apache-2.0 OR BSD-2-Clause | runtime |
| pefile | 2024.8.26 | MIT（人工核实） | build |
| pikepdf | 10.17.0 | MPL-2.0 | runtime |
| pillow | 13.0.0 | MIT-CMU | runtime |
| protobuf | 7.36.2 | BSD-3-Clause | runtime |
| pydantic | 2.14.3 | MIT | runtime |
| pydantic-core | 2.50.4 | MIT | runtime |
| pyinstaller | 6.22.3 | GPL-2.0-or-later WITH Bootloader-exception（人工核实） | build |
| pyinstaller-hooks-contrib | 2026.9 | Apache-2.0 AND GPL-2.0-or-later（人工核实） | build |
| pypdfium2 | 5.15.1 | BSD-3-Clause AND Apache-2.0（人工核实） | runtime |
| python-docx | 1.2.0 | MIT | runtime |
| pywin32-ctypes | 0.2.3 | BSD-3-Clause（人工核实） | build |
| pyyaml | 6.0.3 | MIT | runtime |
| setuptools | 84.2.1 | MIT | build |
| starlette | 1.10.0 | BSD-3-Clause | runtime |
| tokenizers | 0.23.3 | Apache-2.0 | runtime |
| tqdm | 4.70.1 | MPL-2.0 AND MIT | runtime |
| truststore | 0.10.5 | MIT | runtime |
| typing-extensions | 4.16.0 | PSF-2.0 | runtime |
| typing-inspection | 0.4.5 | MIT | runtime |
| uvicorn | 0.54.0 | BSD-3-Clause | runtime |
