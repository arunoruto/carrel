# Changelog

## [0.2.0](https://github.com/arunoruto/carrel/compare/v0.1.0...v0.2.0) (2026-10-31)


### Features

* a logo: a reading nook with three books ([b7b6d06](https://github.com/arunoruto/carrel/commit/b7b6d0691059cb412de9123e62c57ce21ef25719))
* ADS bibcodes, old keys as aliases, long PDFs past Docling ([c578007](https://github.com/arunoruto/carrel/commit/c5780075baa26f4be135cf083d6f73d834466372))
* **bib:** interoperate with JabRef: file links and groups both ways ([cb2ea43](https://github.com/arunoruto/carrel/commit/cb2ea432b854155c5ad7776dbf88981991064ac7))
* books by ISBN, and DOIs that belong to another work ([5e57a9c](https://github.com/arunoruto/carrel/commit/5e57a9c10b84410e74c9b3acf66d97bc3b47a67b))
* carrel serve, with a Docker image and a NixOS module ([5e9d23b](https://github.com/arunoruto/carrel/commit/5e9d23b27b5d514b1056c84057b6743d7e0486c8))
* carrel web, the library in the browser ([124bd15](https://github.com/arunoruto/carrel/commit/124bd15e551e379960c020c7ce544563f79f6485))
* citation graph with interchangeable reference sources ([7b2d15f](https://github.com/arunoruto/carrel/commit/7b2d15f0fc3e2c816f575fb3b8766bfb0e2c2cb9))
* citation tree and confirmed people ([f220393](https://github.com/arunoruto/carrel/commit/f2203935a61a772ab3adfc71f8cf1087b547fb5b))
* **cli:** bib --files points entries at their PDFs for JabRef ([2125460](https://github.com/arunoruto/carrel/commit/2125460d7ccff25d076506471508431b1a753c13))
* **cli:** list and show papers as JSON ([7fb26a4](https://github.com/arunoruto/carrel/commit/7fb26a40afbd53d36debd421f582798746537c66))
* **extract:** Docling from Home Manager or a docling-serve ([5caa2af](https://github.com/arunoruto/carrel/commit/5caa2afa1810a58a1ececaf39ef26dded1544b41))
* **extract:** extractors behind a trait, Docling first when installed ([e7ffa5a](https://github.com/arunoruto/carrel/commit/e7ffa5a85f16c1c3aa0742f5bbdb25fe54ead5a5))
* fill in missing DOIs from ADS before looking up references ([dd86132](https://github.com/arunoruto/carrel/commit/dd86132727405ce9b0c77c70db2aeabbfee50a77))
* Home Manager module that watches Downloads for new papers ([b4e4471](https://github.com/arunoruto/carrel/commit/b4e4471c4269bf0628ee927d7eaee81f9d1961d9))
* import an I, Librarian library ([0b0491f](https://github.com/arunoruto/carrel/commit/0b0491fa217a4c47a5a0cdc26e379eb4a1b49c3d))
* install script, background services and agent integrations ([4cb18f3](https://github.com/arunoruto/carrel/commit/4cb18f3d8624e9cd6c75ffa52b328c1251be41ab))
* let trusted networks use carrel serve without a token ([f0dd165](https://github.com/arunoruto/carrel/commit/f0dd1652e3afefd83e2f754373330eb6d5ebcc03))
* **map:** authors as hubs, clusters; doctor: duplicates without a DOI ([02107f1](https://github.com/arunoruto/carrel/commit/02107f1b1bb6ed809dd85389db04e446ed06240a))
* **map:** institutions, links side by side, clearer arrows ([6f7d222](https://github.com/arunoruto/carrel/commit/6f7d2220f13e5c813305cfd35b518a3e687d54d4))
* merge reference sources and add Crossref ([0cff23b](https://github.com/arunoruto/carrel/commit/0cff23b5f99d68969872beb0b7809b967fcb3719))
* **notify:** desktop and Telegram notifiers ([f86db26](https://github.com/arunoruto/carrel/commit/f86db26f4573656c4aaa67bda195b55bf0ebb598))
* read API tokens from the config ([f4dc88f](https://github.com/arunoruto/carrel/commit/f4dc88f7bf770b17c1a72bcb8cfc0b63a498374d))
* semantic search with Ollama, consume from Downloads, email notifications ([d1e2026](https://github.com/arunoruto/carrel/commit/d1e20265196279e777a95ccd545c09aaa7d074ee))
* share the library through S3 or a folder with carrel sync ([47b3766](https://github.com/arunoruto/carrel/commit/47b37667132a8a6b2dcede58b0b355b679857864))
* tell authors apart by their co-authors ([a73a680](https://github.com/arunoruto/carrel/commit/a73a680ac1a7a7eb04588241d52e15ec9909f795))
* trust X-Forwarded-For from the proxies named in [serve] proxies ([880ae5a](https://github.com/arunoruto/carrel/commit/880ae5ae6d4b450dba28094b8e1462fe88059820))
* **tui:** Tapedeck-style redesign with themes, graph and people tabs ([25ff180](https://github.com/arunoruto/carrel/commit/25ff1803ac2dc1ff3c67c12b4f2a26898bcf3a92))
* **web:** --get-url prints the login link ([0f9dc42](https://github.com/arunoruto/carrel/commit/0f9dc424c4fef8d955b4f4f89fb354f8186d2ff6))
* **web:** a map of the library, people, citation trees and library health ([57ff414](https://github.com/arunoruto/carrel/commit/57ff4146fd1068eba7720c40092d972b96ff45f0))
* **web:** read PDFs over the whole page; a calmer paper page ([e54d2d4](https://github.com/arunoruto/carrel/commit/e54d2d43c59352fcccde2000669b190fddbb5812))


### Bug Fixes

* **authors:** match people in a fraction of the time ([36e3b80](https://github.com/arunoruto/carrel/commit/36e3b808c5629a433b6a851da3b93f72d59efc48))
* consume only finished PDFs; [@string](https://github.com/string) offsets with non-ASCII text ([6a38b5b](https://github.com/arunoruto/carrel/commit/6a38b5bfa1cc837da26f33422a8812c02a36ee2d))
* duplicate PDFs, old keys in rename and remove, review findings ([92c7e69](https://github.com/arunoruto/carrel/commit/92c7e695f30c92cc16e3dd931f812e1e8a64432f))
* **extract:** Docling 2.118's figures, and no NUL bytes in its text ([98ed8a9](https://github.com/arunoruto/carrel/commit/98ed8a92a1720a30cc04b9fb46188c4e42eb1636))
* **extract:** Docling's pdfium parser where its own gives up, else PyMuPDF ([729e24b](https://github.com/arunoruto/carrel/commit/729e24b7f7b30029250e36fca5cd9b91cc6036ae))
* sandbox files served by carrel web; don't let Ollama hold up the sidebar ([2e775f5](https://github.com/arunoruto/carrel/commit/2e775f57b9979b9688b7a5d2dec27f4b6d9c1cf0))
* show the PDF on laptop-sized windows; follow search links in the library ([06192bd](https://github.com/arunoruto/carrel/commit/06192bd3e51fe8d5e739e15a58f3c61b13aa8b80))
* **sync:** validate remote keys, lock the library, refuse an empty remote ([2a917a4](https://github.com/arunoruto/carrel/commit/2a917a4cf111ed593daf3b8ef78436d67eb62183))
* **sync:** wait a moment for the sync lock ([45296f9](https://github.com/arunoruto/carrel/commit/45296f91d5cf4ef88972e8d791a57ca0429bd43b))
* unique staging folders when several papers are added at once ([f6f5e05](https://github.com/arunoruto/carrel/commit/f6f5e05494a01a614cea94e83740c347f3d0f90e))
* **web:** no forms from drafts, a CSP, private upload folders ([217e8d3](https://github.com/arunoruto/carrel/commit/217e8d35c76afd0e1eedfdba0cdab38c90ab0a1a))

## 0.1.0 (2026-10-17)


### Features

* carrel, a research paper library for you and your AI ([be528e8](https://github.com/arunoruto/carrel/commit/be528e89254d9a72fa606133dd3f4768eae8593d))


### Bug Fixes

* honor XDG_CONFIG_HOME and XDG_CACHE_HOME on macOS ([ff162c8](https://github.com/arunoruto/carrel/commit/ff162c8a2aa74f765a9a2b1e386145f3cf7b2429))
