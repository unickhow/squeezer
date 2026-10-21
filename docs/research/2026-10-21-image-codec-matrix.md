# Image Codec Compatibility Matrix

Status: implementation decision record for the image-format coverage work.

Tinify currently lists JPEG, PNG, WebP, AVIF, JPEG XL (JXL), and APNG. Squeezer remains local and preserves the source format; support means intake, preview, optimization, validation, and output in that same format.

| Format | Decode / inspect | Encode / optimize candidate | Alpha / animation | Release and license notes |
| --- | --- | --- | --- | --- |
| JPEG/JPG | Existing `image` JPEG decoder; current macOS `sips` path | Keep ImageIO/`sips` JPEG encoder and quality search | No alpha; preserve EXIF orientation and compatible metadata behavior | Already packaged with macOS; verify all supported output qualities and orientation. |
| PNG | Existing `png` / `image` | `quantette` 0.6 (MIT OR Apache-2.0) + `oxipng`; current in-house alpha-aware palette mapper remains for PNGs with alpha | Opaque PNG uses Oklab k-means and Floyd–Steinberg dithering. Alpha PNG uses a dedicated alpha-aware palette and dithering path. APNG remains separate. | Do not adopt libimagequant (GPLv3-or-later or commercial) under the selected permissive-license direction. `quantette` is the permissive perceptual quantizer for opaque images; transparent-image quality still needs broader fixture benchmarking. |
| WebP | `image` / image-webp pure-Rust decoder | Added `webp` 0.3.1, backed by libwebp; quality-controlled lossy encoding with alpha | Still alpha supported; animation out of scope | Crate uses MIT OR Apache-2.0; libwebp is built as a Cargo dependency for offline packaging. Release-target build remains to verify. |
| AVIF | `image` AVIF decoder with native codec dependencies, or a self-contained Rust decoder | `ravif` / rav1e pure-Rust encoder | Alpha supported for still images | `image/avif-native` currently fails to build here because it needs system `pkg-config`/dav1d. The pure-Rust `avif-decode` 3.0 candidate declares Rust 1.98, newer than this project's toolchain, so it cannot currently be adopted without raising MSRV. |
| JXL | `jxl-oxide` Rust decoder (MIT OR Apache-2.0) | `gamut-jxl` 0.4 / `gamut-jxl-sys` wrapping BSD-3-Clause libjxl | Still alpha supported; animation needs separate validation | The encoder candidate uses upstream libjxl but its static build requires CMake and a C++ toolchain. CMake is absent from the current environment. Its bundled third-party license notices also need release-bundle review. Do not adopt AGPL/commercial `jxl-encoder`. |
| APNG | `png` animation reader with `FrameControl` | Re-encode frames with `png` animation writer while preserving every frame-control field; optimize each frame's PNG encoding | Must preserve frame count, per-frame delay, blend/dispose operations, alpha and canvas size | Do not pass APNG through oxipng 9.1.5: its parser source explicitly warns that it strips animation data. Use the `png` crate's APNG API and verify round-trip behavior. |

## Evidence and Decision

- Current source only accepts JPEG, PNG, and JPEG aliases are handled in the analyzer; the dialog and output path also limit formats.
- `image` documents AVIF, WebP, JPEG and PNG support, but its upstream pure-Rust WebP encoder is lossless-only. Squeezer now uses the `webp` crate/libwebp adapter for quality-controlled lossy encoding.
- `ravif` is a pure-Rust AVIF encoder. `quantette` provides perceptual Oklab quantization, k-means, dithering, and a MIT OR Apache-2.0 license; Squeezer uses it for opaque PNGs and retains an alpha-aware local path for transparent PNGs. The GPL/commercial `imagequant` path is rejected by the owner's permissive-license decision.
- A current pure-Rust AVIF decoder candidate (`avif-decode` 3.0) has no C codec dependency but declares Rust 1.98, which is newer than this project's current Rust toolchain. `image/avif-native` requires system `pkg-config`/dav1d here and is not suitable for the current offline packaged build.
- `jxl-encoder` 0.3.1 supports lossy and lossless still images plus animation, but is AGPL-or-commercial and is rejected by the owner's permissive-license decision. `gamut-jxl` wraps libjxl, whose upstream project is BSD-style; the binding's build script compiles bundled libjxl through CMake. Since CMake is unavailable in this build environment, the candidate is not yet integrated or verified for clean macOS release builds.
- Although oxipng 9.1.5 documents an APNG parser/frame recompression path, its local parser emits "Stripping animation data from APNG" for the `acTL` chunk and the writer does not recreate that control chunk. It is unsafe for preserving animation; use the `png` crate's frame-control-aware reader/writer instead.
- JXL is the largest still-format risk: the selected encoder must be proven on representative files and in the macOS release bundle before the format is shown as fully supported.

## Links

- [Tinify supported image formats](https://tinypng.com/?type=standard)
- [Rust image crate formats and feature flags](https://github.com/image-rs/image)
- [image-webp decoder and encoder status](https://github.com/image-rs/image-webp)
- [libimagequant Rust crate and license](https://docs.rs/crate/imagequant/latest)
- [Quantette palette quantization API and license](https://docs.rs/crate/quantette/latest)
- [ravif encoder documentation](https://docs.rs/ravif/latest/ravif/)
- [gamut-jxl wrapper build and encoder documentation](https://docs.rs/gamut-jxl/latest/gamut_jxl/)
- [jxl-encoder project and quality API](https://github.com/imazen/jxl-encoder)
- [jxl-encoder crate documentation and license](https://docs.rs/crate/jxl-encoder/0.3.1)
- [avif-decode crate documentation and MSRV](https://docs.rs/crate/avif-decode/3.0.0)
- [png crate animation API](https://docs.rs/png/0.18.1/png/)

## Implementation status

Squeezer currently has working JPEG, PNG, and still-WebP compression paths. Opaque PNG now uses quantette's Oklab k-means palette generation with dithering; transparent PNG uses the alpha-aware local palette mapper with dithering. APNG is detected and rejected safely until animation-preserving optimization is implemented. AVIF and JXL are admitted by the format registry but do not yet have a complete same-format compression path. This work is incomplete; the format registry must not be treated as proof of full compression support.
