# Android JVM support

The following dependencies are bundled unchanged in the Android system APK.
Versions, upstream binary URLs and SHA-256 checksums are pinned in
`apps/android/dependencies.json`; the source tree is not vendored.

- rustls-platform-verifier Android 0.2.0: MIT OR Apache-2.0.
  Copyright (c) 2022 1Password. See `rustls-platform-verifier-MIT.txt` and
  `APACHE-2.0.txt`. Source: https://github.com/rustls/rustls-platform-verifier
  Distribution commit: `1aa691352a5e0c215210dfc9a3c2f2078639dc91`.
  `classes.jar` and `res/xml/network_security_config.xml` are extracted unchanged.
  License texts are copied unchanged from the Rust 0.7.1 crate, source commit
  `252e25161a91af476cbea620e29d277592d27ec7`.
- Kotlin standard library 2.2.10: Apache-2.0. See `APACHE-2.0.txt`,
  `kotlin-NOTICE.txt`, and `kotlin-COPYRIGHT.txt`, copied unchanged from
  https://github.com/JetBrains/kotlin/tree/c448af19ded1b1a4e96e9af6412cd9acb100ce1a/license
  (`v2.2.10`). No modifications to those notice files.
- JetBrains annotations 13.0: Apache-2.0. See `APACHE-2.0.txt`.
  Official artifact: https://repo.maven.apache.org/maven2/org/jetbrains/annotations/13.0/ .

Rust dependencies are fixed by the workspace Cargo.lock; their manifests and
distributed license files identify their separate license terms.
